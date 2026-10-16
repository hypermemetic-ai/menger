use super::*;
// SCALE-4: a ticket's children are read by `list { parent }` now.
use super::tests_children::children;

/// MENGER-13: a write's changes as the tests written before the transition
/// log read them — each `Transition` (said right after its ticket's event)
/// set aside. The transitions and where they stand are `tests_history`'s.
pub(super) trait Changes {
    fn try_change(&mut self) -> Result<Change, broadcast::error::TryRecvError>;
    async fn change(&mut self) -> Result<Change, broadcast::error::RecvError>;
}

impl Changes for broadcast::Receiver<Change> {
    fn try_change(&mut self) -> Result<Change, broadcast::error::TryRecvError> {
        loop {
            match self.try_recv()? {
                Change::Transition { .. } => continue,
                c => return Ok(c),
            }
        }
    }

    async fn change(&mut self) -> Result<Change, broadcast::error::RecvError> {
        loop {
            match self.recv().await? {
                Change::Transition { .. } => continue,
                c => return Ok(c),
            }
        }
    }
}

fn new_ticket(epic: &str, title: &str) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind: Kind::Build,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        body: String::new(),
    }
}

/// `new_ticket` as a shortcode's scope root — every prefix starts
/// with one (MNG-12).
fn scope(epic: &str, title: &str) -> NewTicket {
    NewTicket {
        path: None,
        kind: Kind::Scope,
        ..new_ticket(epic, title)
    }
}

/// `new_ticket`, filed as an output of `parent`.
fn child_of(epic: &str, title: &str, parent: &str) -> NewTicket {
    NewTicket {
        path: None,
        parent: Some(parent.into()),
        ..new_ticket(epic, title)
    }
}

fn ids(page: &Page) -> Vec<&str> {
    page.tickets.iter().map(|t| t.id.as_str()).collect()
}

/// MNG-9: the sorted index is the order `list` always had — walking it
/// in pages of one, from no cursor to a null `next`, visits every
/// ticket exactly once, in `(epic, seq)` order, and `remaining` counts
/// down exactly. Ids are created out of order on purpose, and one epic
/// passes 9 so the difference between numeric `seq` and a string
/// compare on the id shows (`A-10` must follow `A-9`, not `A-1`).
#[tokio::test]
async fn the_index_pages_the_whole_store_in_order_exactly_once() {
    let store = Store::new();
    store.create(SYS, scope("A", "a")).await.unwrap(); // MNG-12: the root first
    for _ in 0..10 {
        store.create(SYS, new_ticket("A", "a")).await.unwrap();
    }
    store.create(SYS, scope("B", "b")).await.unwrap();
    store.create(SYS, scope("AB", "ab")).await.unwrap();

    let all: Vec<String> = store.list(SYS, None, None, None).await.unwrap().iter().map(|t| t.id.to_string()).collect();
    assert_eq!(
        all,
        ["A-1", "A-2", "A-3", "A-4", "A-5", "A-6", "A-7", "A-8", "A-9", "A-10", "A-11",
         "AB-1", "B-1"],
        "seq is numeric (A-10 after A-9), epic is lexicographic"
    );

    let mut walked: Vec<String> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = store
            .page(SYS, PageQuery {
                after: after.as_deref(),
                limit: Some(1),
                ..PageQuery::default()
            })
            .unwrap();
        assert_eq!(page.remaining as usize, all.len() - walked.len() - page.tickets.len());
        walked.extend(page.tickets.iter().map(|t| t.id.to_string()));
        match page.next {
            Some(next) => {
                assert_eq!(Some(&next), walked.last(), "the cursor is the last row");
                after = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(walked, all, "one page at a time == the whole list, in order");
}

/// MNG-9: `after` composes with the filters, `remaining` counts under
/// them, and a cursor naming no ticket is the cursor error (not an
/// empty page) — the message the wire shows.
#[tokio::test]
async fn a_page_composes_with_filters_and_a_bad_cursor_is_an_error() {
    let store = Store::new();
    for i in 0..6 {
        let nt = if i == 0 { scope("PG", "p") } else { new_ticket("PG", "p") };
        let id = store.create(SYS, nt).await.unwrap().id;
        if i % 2 == 0 {
            store.set_status(SYS.permit(&id), Status::Ready).await.unwrap();
        }
    }
    store.create(SYS, scope("ZZ", "z")).await.unwrap();

    let page = store
        .page(SYS, PageQuery {
            status: Some(Status::Ready),
            after: Some("pg-1"),
            limit: Some(1),
            ..PageQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&page), ["PG-3"], "the cursor is case-insensitive, like every id");
    assert_eq!((page.next.as_deref(), page.remaining), (Some("PG-3"), 1));

    let page = store
        .page(SYS, PageQuery {
            epic: Some("PG"),
            after: Some("PG-6"),
            ..PageQuery::default()
        })
        .unwrap();
    assert!(page.tickets.is_empty(), "ZZ-1 is past the epic filter");
    assert_eq!((page.next, page.remaining), (None, 0));

    match store.page(SYS, PageQuery {
        after: Some("ZZZ-9"),
        ..PageQuery::default()
    }) {
        Err(StoreError::CursorNotFound(id)) => {
            assert_eq!(id, "ZZZ-9");
            assert_eq!(
                StoreError::CursorNotFound(id).to_string(),
                "after: no ticket ZZZ-9"
            );
        }
        other => panic!("expected CursorNotFound, got {other:?}"),
    }
}

/// MNG-9: an epic is a *position*, not a predicate — `(epic, seq)`
/// puts an epic's tickets in one contiguous stretch of the index, so
/// an epic-filtered page is bounded by that stretch. The cursor can
/// sit below it, inside it, or past it; all three answer correctly.
#[tokio::test]
async fn an_epic_filter_bounds_the_walk_wherever_the_cursor_sits() {
    let store = Store::new();
    for epic in ["AA", "MM", "ZZ"] {
        store.create(SYS, scope(epic, "t")).await.unwrap();
        for _ in 0..2 {
            store.create(SYS, new_ticket(epic, "t")).await.unwrap();
        }
    }
    let epic_page = |after: Option<&'static str>, limit| {
        store.page(SYS, PageQuery {
            epic: Some("MM"),
            after,
            limit,
            ..PageQuery::default()
        })
    };

    // Cursor below the epic: the epic's own lower bound stands.
    let page = epic_page(Some("AA-2"), Some(2)).unwrap();
    assert_eq!(ids(&page), ["MM-1", "MM-2"]);
    assert_eq!((page.next.as_deref(), page.remaining), (Some("MM-2"), 1));

    // Cursor inside the epic: it wins.
    let page = epic_page(Some("MM-2"), None).unwrap();
    assert_eq!(ids(&page), ["MM-3"]);
    assert_eq!((page.next, page.remaining), (None, 0));

    // Cursor past the epic: nothing is left, and nothing from ZZ leaks.
    let page = epic_page(Some("ZZ-1"), None).unwrap();
    assert!(page.tickets.is_empty());
    assert_eq!((page.next, page.remaining), (None, 0));

    // And the whole epic, uncursored, is still just the epic.
    let page = epic_page(None, None).unwrap();
    assert_eq!(ids(&page), ["MM-1", "MM-2", "MM-3"]);

    // A cursor that names nothing still fails, epic or no epic.
    assert!(matches!(
        epic_page(Some("QQ-1"), None),
        Err(StoreError::CursorNotFound(_))
    ));
}

/// MNG-9: a ticket created above a walk's cursor is visited by the
/// walk (the index is live — the cursor is a position in the order,
/// not a snapshot); one below it is not, which is what `watch` is for.
#[tokio::test]
async fn a_ticket_born_above_the_cursor_is_still_walked() {
    let store = Store::new();
    store.create(SYS, scope("MID", "m")).await.unwrap();
    for _ in 0..2 {
        store.create(SYS, new_ticket("MID", "m")).await.unwrap();
    }
    store.create(SYS, scope("AAA", "below")).await.unwrap();

    let first = store
        .page(SYS, PageQuery {
            limit: Some(2),
            ..PageQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&first), ["AAA-1", "MID-1"]);

    // Mid-walk: one above the cursor (MID-4), one below it (AAA-2).
    store.create(SYS, new_ticket("MID", "above")).await.unwrap();
    store.create(SYS, new_ticket("AAA", "below again")).await.unwrap();

    let rest = store
        .page(SYS, PageQuery {
            after: first.next.as_deref(),
            ..PageQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&rest), ["MID-2", "MID-3", "MID-4"], "MID-4 joins the walk");
    assert_eq!((rest.next, rest.remaining), (None, 0));
}

/// MNG-9: `ready_set` pages the same way, on the ready-set membership
/// rule — blocked and non-Ready tickets are not in the order it walks.
#[tokio::test]
async fn the_ready_set_pages_on_its_own_membership() {
    let store = Store::new();
    for i in 0..4 {
        let nt = if i == 0 { scope("RD", "r") } else { new_ticket("RD", "r") };
        let id = store.create(SYS, nt).await.unwrap().id;
        store.set_status(SYS.permit(&id), Status::Ready).await.unwrap();
    }
    store
        .create(SYS, NewTicket {
            path: None,
            blocked_by: vec!["RD-1".into()],
            ..new_ticket("RD", "blocked")
        })
        .await
        .unwrap();
    store.set_status(SYS.permit("RD-5"), Status::Ready).await.unwrap();

    let page = store.ready_page(SYS, None, None, Some(2)).unwrap();
    assert_eq!(ids(&page), ["RD-1", "RD-2"]);
    assert_eq!((page.next.as_deref(), page.remaining), (Some("RD-2"), 2));

    let page = store.ready_page(SYS, None, page.next.as_deref(), Some(2)).unwrap();
    assert_eq!(ids(&page), ["RD-3", "RD-4"], "RD-5 is blocked by RD-1");
    assert_eq!((page.next, page.remaining), (None, 0));
}

/// The store's schema as it was before AGS-13 (`ticket_type`), so a
/// test can build "a database created before this ticket" by hand.
const LEGACY_SCHEMA: &str = "
        CREATE TABLE tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            ticket_type  TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

/// Writes a pre-AGS-13 database at `path` holding one row per
/// `(seq, ticket_type)` in `rows`, epic `LEG`, bodies with frontmatter.
fn write_legacy_db(path: &Path, rows: &[(u32, &str)]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(LEGACY_SCHEMA).unwrap();
    for (seq, tt) in rows {
        conn.execute(
            "INSERT INTO tickets VALUES (?1, 'LEG', ?2, ?3, 'Pending', ?4, NULL, '[]', '[]', ?5, 1, 1)",
            rusqlite::params![
                format!("LEG-{seq}"),
                seq,
                format!("legacy {tt}"),
                tt,
                format!("---\nid: LEG-{seq}\ntype: {tt}\n---\n\nbody"),
            ],
        )
        .unwrap();
    }
}

fn columns(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn.prepare("PRAGMA table_info(tickets)").unwrap();
    stmt.query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// AGS-13 criterion 1 at the store level: every legacy value maps per
/// the pinned table, the rows are rewritten in the file (not remapped
/// per load), and the body's frontmatter is left exactly as it was.
#[tokio::test]
async fn legacy_ticket_type_migrates_durably_on_first_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    write_legacy_db(
        &path,
        &[
            (1, "epic"),
            (2, "implementation"),
            (3, "analysis"),
            (4, "checkpoint"),
            (5, "spike"),
        ],
    );
    assert!(columns(&path).contains(&"ticket_type".to_string()));

    let expected = [
        ("LEG-1", Kind::Scope),
        ("LEG-2", Kind::Build),
        ("LEG-3", Kind::Spike),
        ("LEG-4", Kind::Execution),
        ("LEG-5", Kind::Spike),
    ];
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.count(SYS), 5);
        for (id, kind) in expected {
            let t = store.get(SYS, id).await.unwrap();
            assert_eq!(t.kind, kind, "{id}");
            assert!(t.body.contains("type: "), "{id}: frontmatter untouched");
        }
    }

    // The file itself now speaks kinds: column renamed, values rewritten.
    let cols = columns(&path);
    assert!(cols.contains(&"kind".to_string()), "{cols:?}");
    assert!(!cols.contains(&"ticket_type".to_string()), "{cols:?}");
    {
        let conn = Connection::open(&path).unwrap();
        let raw: Vec<(String, String)> = conn
            .prepare("SELECT id, kind FROM tickets ORDER BY seq")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            raw,
            vec![
                ("LEG-1".into(), "scope".into()),
                ("LEG-2".into(), "build".into()),
                ("LEG-3".into(), "spike".into()),
                ("LEG-4".into(), "execution".into()),
                ("LEG-5".into(), "spike".into()),
            ]
        );
        assert!(!db::has_column(&conn, "tickets", "ticket_type").unwrap());
    }

    // Second open: nothing to migrate, same kinds, still writable.
    let store = Store::open(&path).unwrap();
    for (id, kind) in expected {
        assert_eq!(store.get(SYS, id).await.unwrap().kind, kind, "{id} after reopen");
    }
    let next = store.create(SYS, new_ticket("LEG", "born after the migration")).await.unwrap();
    assert_eq!(next.id, "LEG-6");
    assert_eq!(next.kind, Kind::Build);
}

/// A legacy value outside the pinned mapping refuses the open,
/// naming the row, and leaves the database exactly as it was.
#[test]
fn legacy_value_outside_the_mapping_fails_the_open_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("odd.db");
    write_legacy_db(&path, &[(1, "implementation"), (2, "wibble")]);

    let err = match Store::open(&path) {
        Ok(_) => panic!("an unmappable legacy value must refuse the open"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("LEG-2"), "names the row: {err}");
    assert!(err.contains("'wibble'"), "names the value: {err}");
    assert!(err.contains("implementation->build"), "shows the mapping: {err}");

    // Untouched: the legacy column and both values are still there.
    let cols = columns(&path);
    assert!(cols.contains(&"ticket_type".to_string()), "{cols:?}");
    assert!(!cols.contains(&"kind".to_string()), "{cols:?}");
    let conn = Connection::open(&path).unwrap();
    let tt: String = conn
        .query_row("SELECT ticket_type FROM tickets WHERE id = 'LEG-1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tt, "implementation", "the good row was not rewritten either");
}

// ---- AGS-19: parenthood --------------------------------------------

/// The store's schema as it was before AGS-19 (no `parent`, no
/// `children`), so a test can build "a database created before this
/// ticket" by hand — the black-box counterpart is
/// `tests/fixtures/pre-ags19.db`, written by the pre-AGS-19 binary.
const PRE_AGS19_SCHEMA: &str = "
        CREATE TABLE tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            kind         TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

/// Writes a pre-AGS-19 database at `path`, epic `OLD`, one row per
/// `(seq, frontmatter parent)` — `None` writes a body with no
/// frontmatter at all.
fn write_pre_ags19_db(path: &Path, rows: &[(u32, Option<&str>)]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(PRE_AGS19_SCHEMA).unwrap();
    for (seq, parent) in rows {
        let body = match parent {
            Some(p) => format!(
                "---\nid: OLD-{seq}\ntitle: \"old {seq}\"\nstatus: Ready\nparent: {p}\n---\n\nbody"
            ),
            None => format!("no frontmatter, just prose for OLD-{seq}"),
        };
        conn.execute(
            "INSERT INTO tickets VALUES (?1, 'OLD', ?2, ?3, 'Pending', 'build', NULL, '[]', '[]', ?4, 1, 1)",
            rusqlite::params![format!("OLD-{seq}"), seq, format!("old {seq}"), body],
        )
        .unwrap();
    }
}

/// Acceptance 1 at the store level: a child names its parent, the
/// parent gains it among its children, a subscriber sees the parent's
/// change as `ChildrenChanged` (SCALE-4: its id and new count), and the
/// edge is durable.
#[tokio::test]
async fn a_childs_birth_updates_its_parent_and_is_broadcast() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("family.db");
    let store = Store::open(&path).unwrap();
    let parent = store.create(SYS, scope("FAM", "the parent")).await.unwrap();
    assert_eq!(parent.parent, None);
    assert_eq!(parent.child_count, 0);

    let mut rx = store.subscribe();
    let child = store
        .create(SYS, child_of("FAM", "the output", "FAM-1"))
        .await
        .unwrap();
    assert_eq!(child.parent.as_deref(), Some("FAM-1"));
    assert_eq!(child.child_count, 0);

    match rx.change().await.unwrap() {
        Change::Created { ticket } => assert_eq!(ticket.id, "FAM-2"),
        other => panic!("expected Created, got {other:?}"),
    }
    match rx.change().await.unwrap() {
        Change::ChildrenChanged { id, added, child_count, .. } => {
            assert_eq!(id, "FAM-1");
            assert_eq!(added, "FAM-2");
            // SCALE-4: the parent's id and count, not the parent.
            assert_eq!(child_count, 1);
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    assert_eq!(children(&store, "FAM-1"), vec!["FAM-2"]);
    assert_eq!(store.get(SYS, "FAM-1").await.unwrap().child_count, 1);

    // Durable: a reopen finds the edge, and counts it.
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(children(&store, "FAM-1"), vec!["FAM-2"]);
    assert_eq!(store.get(SYS, "FAM-1").await.unwrap().child_count, 1);
    assert_eq!(store.get(SYS, "FAM-2").await.unwrap().parent.as_deref(), Some("FAM-1"));
}

/// Acceptance 2 at the store level: a parent that names no ticket is
/// refused with the reason and nothing is stored; self-ancestry is
/// impossible by construction (the allocated id cannot already be an
/// ancestor, and there is no re-parent method), and a 3-deep chain —
/// crossing epics — reads back correctly.
#[tokio::test]
async fn nonexistent_parent_is_refused_and_a_three_deep_chain_reads_back() {
    let store = Store::new();
    // MNG-12: the shortcode's scope root comes first, so the orphan
    // would be CHAIN-2.
    store.create(SYS, scope("CHAIN", "root")).await.unwrap();
    let err = store
        .create(SYS, child_of("CHAIN", "orphan", "CHAIN-9"))
        .await
        .unwrap_err();
    match &err {
        StoreError::ParentNotFound { id, parent } => {
            assert_eq!(id, "CHAIN-2");
            assert_eq!(parent, "CHAIN-9");
        }
        other => panic!("expected ParentNotFound, got {other:?}"),
    }
    assert!(err.to_string().contains("CHAIN-9"), "{err}");
    assert!(err.to_string().contains("must exist"), "{err}");
    assert_eq!(store.count(SYS), 1, "nothing was created");

    // A ticket cannot name itself: the id is allocated by this very
    // create, so it cannot exist yet — the store refuses it as a
    // missing parent, before any cycle can be formed. The id stays
    // unallocated: the next successful create still takes CHAIN-2.
    let err = store
        .create(SYS, child_of("CHAIN", "myself", "CHAIN-2"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::ParentNotFound { .. }),
        "self-parenthood is impossible by construction, got {err:?}"
    );

    // Three deep, the middle link in another epic (its scope root).
    store
        .create(SYS, NewTicket {
            path: None,
            kind: Kind::Scope,
            ..child_of("OTHER", "middle", "CHAIN-1")
        })
        .await
        .unwrap();
    // MNG-14: a non-root stays inside its shortcode — the leaf under
    // OTHER-1 is OTHER-2, and a CHAIN ticket naming OTHER-1 is refused.
    let err = store
        .create(SYS, child_of("CHAIN", "leaf", "OTHER-1"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::ParentOutsideShortcode { .. }),
        "cross-shortcode parent on a non-root, got {err:?}"
    );
    assert!(err.to_string().contains("OTHER-1") && err.to_string().contains("outside"), "{err}");
    store
        .create(SYS, child_of("OTHER", "leaf", "OTHER-1"))
        .await
        .unwrap();
    assert_eq!(store.get(SYS, "CHAIN-1").await.unwrap().parent, None);
    assert_eq!(children(&store, "CHAIN-1"), vec!["OTHER-1"]);
    assert_eq!(store.get(SYS, "OTHER-1").await.unwrap().parent.as_deref(), Some("CHAIN-1"));
    assert_eq!(children(&store, "OTHER-1"), vec!["OTHER-2"]);
    assert_eq!(store.get(SYS, "OTHER-2").await.unwrap().parent.as_deref(), Some("OTHER-1"));
    assert!(children(&store, "OTHER-2").is_empty());

    // And the guard itself: an ancestry walk that would come back
    // around is caught before anything persists.
    let tickets = Cache::new(
        store
            .page(SYS, PageQuery::default())
            .unwrap()
            .tickets
            .into_iter()
            .map(|t| (t.id.to_string(), t))
            .collect(),
        Default::default(),
    );
    assert_eq!(
        find_parent_cycle(&tickets, "CHAIN-1", "OTHER-2"),
        Some(vec!["OTHER-2".into(), "OTHER-1".into(), "CHAIN-1".into()]),
        "a chain that reaches the ticket itself is a cycle"
    );
    assert_eq!(find_parent_cycle(&tickets, "CHAIN-3", "OTHER-2"), None);
}

/// A ticket that is both the parent and a blocker of the newborn is
/// updated once and carries both edges — the two events that follow
/// both show it as it now is (PERF-4 is exactly this shape): SCALE-4,
/// `ChildrenChanged` its count, `UnlocksChanged` the whole ticket.
#[tokio::test]
async fn a_parent_that_is_also_a_blocker_gets_both_edges() {
    let store = Store::new();
    store.create(SYS, scope("BOTH", "the parent")).await.unwrap();
    let mut rx = store.subscribe();
    let child = store
        .create(SYS, NewTicket {
            path: None,
            blocked_by: vec!["BOTH-1".into()],
            ..child_of("BOTH", "report", "BOTH-1")
        })
        .await
        .unwrap();
    assert_eq!(child.parent.as_deref(), Some("BOTH-1"));
    assert_eq!(child.blocked_by, vec!["BOTH-1"]);

    let parent = store.get(SYS, "BOTH-1").await.unwrap();
    assert_eq!(children(&store, "BOTH-1"), vec!["BOTH-2"]);
    assert_eq!(parent.child_count, 1);
    assert_eq!(parent.unlocks, vec!["BOTH-2"], "neither edge overwrote the other");

    assert!(matches!(rx.change().await.unwrap(), Change::Created { .. }));
    match rx.change().await.unwrap() {
        Change::ChildrenChanged { id, child_count, .. } => {
            assert_eq!((id.as_str(), child_count), ("BOTH-1", parent.child_count))
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match rx.change().await.unwrap() {
        Change::UnlocksChanged { ticket, .. } => assert_eq!(ticket, parent),
        other => panic!("expected UnlocksChanged, got {other:?}"),
    }
}

/// Acceptance 4 at the store level: `list` filtered by parent returns
/// exactly that ticket's children, ordered (epic, seq), and composes
/// with the other filters.
#[tokio::test]
async fn list_filters_by_parent() {
    let store = Store::new();
    store.create(SYS, scope("LP", "parent")).await.unwrap();
    store.create(SYS, new_ticket("LP", "unrelated")).await.unwrap();
    store.create(SYS, child_of("LP", "child a", "LP-1")).await.unwrap();
    store
        .create(SYS, NewTicket {
            path: None,
            kind: Kind::Scope,
            ..child_of("ZZ", "child b", "LP-1")
        })
        .await
        .unwrap();
    store.create(SYS, child_of("LP", "grandchild", "LP-3")).await.unwrap();
    store.set_status(SYS.permit("LP-3"), Status::Ready).await.unwrap();

    // MNG-14: "unrelated" (LP-2) named no parent, so it defaulted to
    // the shortcode root and is LP-1's child too.
    let ids = |ts: Vec<Ticket>| ts.into_iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(
        ids(store.list(SYS, None, None, Some("LP-1")).await.unwrap()),
        vec!["LP-2", "LP-3", "ZZ-1"],
        "exactly LP-1's children, ordered (epic, seq)"
    );
    assert_eq!(ids(store.list(SYS, None, None, Some("lp-1")).await.unwrap()), vec!["LP-2", "LP-3", "ZZ-1"]);
    assert_eq!(ids(store.list(SYS, None, None, Some("LP-3")).await.unwrap()), vec!["LP-4"]);
    assert!(store.list(SYS, None, None, Some("LP-2")).await.unwrap().is_empty(), "no children");
    assert!(store.list(SYS, None, None, Some("NOPE-1")).await.unwrap().is_empty());
    // Composes with the existing filters.
    assert_eq!(ids(store.list(SYS, Some("LP"), None, Some("LP-1")).await.unwrap()), vec!["LP-2", "LP-3"]);
    assert_eq!(
        ids(store.list(SYS, None, Some(Status::Ready), Some("LP-1")).await.unwrap()),
        vec!["LP-3"]
    );
    assert_eq!(store.list(SYS, None, None, None).await.unwrap().len(), 5, "unfiltered is unchanged");
}

/// Acceptance 3 at the store level: a pre-AGS-19 database gains the
/// columns and its typed parents on first open, from body
/// frontmatter — resolvable ones written through (and, SCALE-4, read
/// back as children), an unresolvable one left empty, a frontmatter cycle
/// refused, a body without frontmatter untouched — and the rewrite is
/// durable, once. Nothing about it is fatal.
#[tokio::test]
async fn pre_ags19_database_backfills_typed_parents_from_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    write_pre_ags19_db(
        &path,
        &[
            (1, None),            // no frontmatter at all
            (2, Some("OLD-1")),   // resolvable
            (3, Some("OLD-99")),  // names nothing: left empty
            (4, Some("OLD-2")),   // resolvable, three deep
            (5, Some("OLD-6")),   // a two-cycle with OLD-6 …
            (6, Some("OLD-5")),   // … one half of which is refused
        ],
    );
    assert!(!columns(&path).contains(&"parent".to_string()));

    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.count(SYS), 6);
        let p = async |id: &str| store.get(SYS, id).await.unwrap().parent;
        assert_eq!(p("OLD-1").await, None);
        assert_eq!(p("OLD-2").await.as_deref(), Some("OLD-1"));
        assert_eq!(p("OLD-3").await, None, "an unresolvable parent is left empty");
        assert_eq!(p("OLD-4").await.as_deref(), Some("OLD-2"));
        // The chain is acyclic: the first half of the OLD-5/OLD-6
        // pair is taken, the half that would close it is refused.
        assert_eq!(p("OLD-5").await.as_deref(), Some("OLD-6"));
        assert_eq!(p("OLD-6").await, None);
        // …and every accepted edge is a child of its parent.
        assert_eq!(children(&store, "OLD-1"), vec!["OLD-2"]);
        assert_eq!(children(&store, "OLD-2"), vec!["OLD-4"]);
        assert_eq!(children(&store, "OLD-6"), vec!["OLD-5"]);
        assert!(children(&store, "OLD-3").is_empty());
        // The body is the raw record, untouched by the migration.
        assert!(store.get(SYS, "OLD-3").await.unwrap().body.contains("parent: OLD-99"));
    }

    // The file itself carries the typed parents now.
    let cols = columns(&path);
    assert!(cols.contains(&"parent".to_string()), "{cols:?}");
    // SCALE-4: the backfill no longer adds a list for the edges to be
    // mirrored in; the cache derives children from `parent`.
    assert!(!cols.contains(&"children".to_string()), "{cols:?}");
    {
        let conn = Connection::open(&path).unwrap();
        let raw: Vec<(String, Option<String>)> = conn
            .prepare("SELECT id, parent FROM tickets ORDER BY seq")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            raw,
            vec![
                ("OLD-1".into(), None),
                ("OLD-2".into(), Some("OLD-1".into())),
                ("OLD-3".into(), None),
                ("OLD-4".into(), Some("OLD-2".into())),
                ("OLD-5".into(), Some("OLD-6".into())),
                ("OLD-6".into(), None),
            ]
        );
    }

    // Second open: nothing to migrate, same parents, still writable —
    // and the newborn's own parent goes through the normal path.
    let store = Store::open(&path).unwrap();
    assert_eq!(db::migrate_add_parenthood(&mut Connection::open(&path).unwrap()).unwrap(), 0);
    assert_eq!(store.get(SYS, "OLD-4").await.unwrap().parent.as_deref(), Some("OLD-2"));
    let next = store.create(SYS, child_of("OLD", "born after", "OLD-4")).await.unwrap();
    assert_eq!(next.id, "OLD-7");
    assert_eq!(next.parent.as_deref(), Some("OLD-4"));
    assert_eq!(children(&store, "OLD-4"), vec!["OLD-7"]);
}

/// The frontmatter reader: only a leading block counts, only the
/// `parent:` key, and anything empty means "no parent".
#[test]
fn frontmatter_parent_reads_only_a_leading_block() {
    let p = db::frontmatter_parent;
    assert_eq!(p("---\nid: A-1\nparent: B-2\n---\n\nbody"), Some("B-2".into()));
    assert_eq!(p("---\nparent: \"B-2\"\n---\n"), Some("B-2".into()));
    assert_eq!(p("---\nparent: b-2\n---\n"), Some("B-2".into()));
    assert_eq!(p("---\nid: A-1\n---\n\nparent: B-2 in the prose"), None);
    assert_eq!(p("no frontmatter\nparent: B-2\n"), None);
    assert_eq!(p("---\nparent:\n---\n"), None);
    assert_eq!(p("---\nparent: []\n---\n"), None);
    assert_eq!(p("---\nblocked_by: [B-2]\n---\n"), None);
    assert_eq!(p(""), None);
}

#[test]
fn unknown_kind_error_names_the_six() {
    let err = "bug".parse::<Kind>().unwrap_err();
    for name in ["scope", "execution", "build", "gap", "spike", "doc"] {
        assert!(err.contains(name), "{err}");
    }
    assert!(err.contains("'bug'"), "{err}");
    assert_eq!("GAP".parse::<Kind>(), Ok(Kind::Gap), "case-insensitive");
}

#[tokio::test]
async fn reopen_round_trips_every_field_and_continues_seq() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tickets.db");

    let (after_flip, child) = {
        let store = Store::open(&path).unwrap();
        store
            .create(SYS, NewTicket {
                path: None,
                epic: "rt".into(),
                title: "first — ünïcode ✓".into(),
                kind: Kind::Scope,
                severity: Some(Severity::Critical),
                blocked_by: vec!["RT-0".into(), "X-9".into()],
                parent: None,
                body: "## Problem\n\nlines\nand more".into(),
            })
            .await
            .unwrap();
        // AGS-19: the parent edge and its mirror ride the same path.
        let child = store
            .create(SYS, child_of("RT", "its output", "RT-1"))
            .await
            .unwrap();
        let parent = match store.set_status(SYS.permit("RT-1"), Status::Ready).await.unwrap() {
            Change::StatusChanged { ticket, previous } => {
                assert_eq!(previous, Status::Pending);
                ticket
            }
            other => panic!("expected StatusChanged, got {other:?}"),
        };
        assert_eq!(parent.child_count, 1);
        (parent, child)
    };

    // Restart: same location, everything identical.
    let store = Store::open(&path).unwrap();
    assert_eq!(store.count(SYS), 2);
    assert_eq!(store.get(SYS, "RT-1").await.unwrap(), after_flip);
    assert_eq!(store.get(SYS, "RT-2").await.unwrap(), child);
    assert_eq!(store.get(SYS, "RT-2").await.unwrap().parent.as_deref(), Some("RT-1"));

    // Id allocation continues from the persisted max, not from 1.
    let next = store.create(SYS, new_ticket("RT", "second")).await.unwrap();
    assert_eq!(next.id, "RT-3");
    assert_eq!(next.seq, 3);
    assert_eq!(next.parent.as_deref(), Some("RT-1"), "MNG-14: defaulted to the root");
    assert_eq!(next.child_count, 0);
    assert_eq!(children(&store, "RT-1"), vec!["RT-2", "RT-3"]);
}

#[tokio::test]
async fn fresh_location_is_an_empty_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("brand-new.db")).unwrap();
    assert_eq!(store.count(SYS), 0);
    assert!(store.create(SYS, scope("NEW", "works")).await.is_ok());
}

// ---- PERF-2: synthetic seeding -------------------------------------

fn spec(count: usize, seed: u64) -> SeedSpec {
    SeedSpec {
        count,
        seed,
        into_existing: false,
    }
}

/// Iterative DFS over `blocked_by`, reporting the first back edge.
fn has_cycle(tickets: &HashMap<String, Ticket>) -> Option<String> {
    // 0 = unvisited, 1 = on the stack, 2 = done.
    let mut color: HashMap<&str, u8> = HashMap::with_capacity(tickets.len());
    for start in tickets.keys() {
        if color.get(start.as_str()).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(start.as_str(), 0)];
        color.insert(start.as_str(), 1);
        while let Some((id, next)) = stack.pop() {
            let edges = &tickets[id].blocked_by;
            if next < edges.len() {
                stack.push((id, next + 1));
                let b = edges[next].as_str();
                match color.get(b).copied().unwrap_or(0) {
                    0 => {
                        if tickets.contains_key(b) {
                            color.insert(b, 1);
                            stack.push((b, 0));
                        }
                    }
                    1 => return Some(format!("{id} -> {b}")),
                    _ => {}
                }
            } else {
                color.insert(id, 2);
            }
        }
    }
    None
}

/// PERF-2 criterion 1 at the store level, on 10⁴: the count, a
/// realistic spread (several epics, every status and kind present),
/// a modest fraction with blockers, and the DAG invariants — no
/// dangling blocker, `unlocks` mirrors every `blocked_by` exactly,
/// no cycle — plus `ready_set` and `blockers` answering correctly
/// against an independent computation. And it all persisted: a
/// reopen of the file loads the same tickets.
#[tokio::test]
async fn seed_ten_thousand_has_a_valid_dag_and_correct_queries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seeded.db");
    let store = Store::open(&path).unwrap();
    let report = store.seed_synthetic(SYS, spec(10_000, 42)).unwrap();
    assert_eq!(report.count, 10_000);
    assert_eq!(store.count(SYS), 10_000);
    assert!(report.epics >= 10, "several epics, got {}", report.epics);
    assert!(report.edges > 0);

    let all = store.list(SYS, None, None, None).await.unwrap();
    assert_eq!(all.len(), 10_000);
    let by_id: HashMap<String, Ticket> = all.iter().map(|t| (t.id.to_string(), t.clone())).collect();

    // Shape: every status and kind occurs; ids are EPIC-N with N
    // contiguous per epic from 1.
    for s in [
        Status::Pending,
        Status::Ready,
        Status::Blocked,
        Status::Complete,
        Status::Idea,
        Status::Epic,
        Status::Superseded,
    ] {
        assert!(all.iter().any(|t| t.status == s), "status {s:?} present");
    }
    for k in [Kind::Scope, Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike] {
        assert!(all.iter().any(|t| t.kind == k), "kind {k:?} present");
    }
    let mut per_epic: HashMap<&str, Vec<u32>> = HashMap::new();
    for t in &all {
        assert_eq!(t.id, format!("{}-{}", t.epic, t.seq));
        per_epic.entry(t.epic.as_str()).or_default().push(t.seq);
    }
    assert_eq!(per_epic.len(), report.epics);
    for (epic, mut seqs) in per_epic {
        seqs.sort_unstable();
        assert_eq!(seqs, (1..=seqs.len() as u32).collect::<Vec<_>>(), "{epic} ids contiguous");
    }
    let with_blockers = all.iter().filter(|t| !t.blocked_by.is_empty()).count();
    assert!(
        with_blockers * 100 / all.len() >= 10 && with_blockers * 100 / all.len() <= 40,
        "a modest fraction carries blockers, got {with_blockers}/10000"
    );
    assert!(all.iter().all(|t| !t.body.is_empty() && t.body.starts_with("## Problem")));

    // DAG invariants.
    let mut edges = 0;
    for t in &all {
        for b in &t.blocked_by {
            let blocker = by_id.get(b.as_str()).unwrap_or_else(|| panic!("{}: dangling blocker {b}", t.id));
            assert!(blocker.unlocks.iter().any(|x| *x == t.id), "{b}.unlocks lacks {}", t.id);
            edges += 1;
        }
        for u in &t.unlocks {
            let dependent = by_id.get(u.as_str()).unwrap_or_else(|| panic!("{}: unlocks names {u}", t.id));
            assert!(dependent.blocked_by.iter().any(|x| *x == t.id), "{u}.blocked_by lacks {}", t.id);
        }
    }
    assert_eq!(edges, report.edges);
    assert_eq!(has_cycle(&by_id), None);

    // ready_set against an independent computation over `list`.
    let expected: Vec<String> = all
        .iter()
        .filter(|t| t.status == Status::Ready)
        .filter(|t| t.blocked_by.iter().all(|b| satisfied(by_id[b.as_str()].status)))
        .map(|t| t.id.to_string())
        .collect();
    let got: Vec<String> = store.ready_set(SYS, None).await.unwrap().into_iter().map(|t| t.id.into_string()).collect();
    assert!(!got.is_empty());
    assert_eq!(got, expected);
    // ...and it is doing real work: some Ready tickets are held back.
    assert!(
        all.iter().filter(|t| t.status == Status::Ready).count() > got.len(),
        "some Ready tickets have unsatisfied blockers"
    );

    // blockers on a sampled ticket with several blockers.
    let sample = all.iter().filter(|t| t.blocked_by.len() >= 2).nth(7).expect("a multi-blocker ticket");
    let refs = store.blockers(SYS, &sample.id).await.unwrap();
    assert_eq!(refs.len(), sample.blocked_by.len());
    for (r, b) in refs.iter().zip(&sample.blocked_by) {
        match r {
            BlockerRef::Resolved(t) => assert_eq!(&t.id, b),
            BlockerRef::Dangling(d) => panic!("{}: {d} dangling", sample.id),
            BlockerRef::Hidden => panic!("{}: a blocker hidden from the system", sample.id),
        }
    }

    // Persisted: a reopen finds the same tickets (write-through happened).
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.count(SYS), 10_000);
    assert_eq!(reopened.list(SYS, None, None, None).await.unwrap(), all);
}

/// Criterion 4: the same seed value produces byte-identical tickets;
/// a different one produces different tickets.
#[tokio::test]
async fn seed_is_deterministic_per_seed_value() {
    let a = Store::new();
    let b = Store::new();
    let c = Store::new();
    a.seed_synthetic(SYS, spec(500, 7)).unwrap();
    b.seed_synthetic(SYS, spec(500, 7)).unwrap();
    c.seed_synthetic(SYS, spec(500, 8)).unwrap();
    let la = a.list(SYS, None, None, None).await.unwrap();
    assert_eq!(la, b.list(SYS, None, None, None).await.unwrap());
    assert_ne!(la, c.list(SYS, None, None, None).await.unwrap());
    assert_eq!(serde_json::to_string(&la).unwrap(), serde_json::to_string(&b.list(SYS, None, None, None).await.unwrap()).unwrap());
}

/// Criterion 3: a store that already holds tickets refuses the seed
/// (nothing written) unless explicitly allowed — and then ids
/// continue after the existing maxima, never colliding.
#[tokio::test]
async fn seed_refuses_an_existing_store_unless_allowed_then_continues_ids() {
    let store = Store::new();
    store.create(SYS, scope("REAL", "already here")).await.unwrap();

    let err = store.seed_synthetic(SYS, spec(100, 1)).unwrap_err();
    assert!(matches!(err, StoreError::SeedRefused { existing: 1 }), "{err}");
    assert!(err.to_string().contains("already holds 1 ticket"), "{err}");
    assert_eq!(store.count(SYS), 1, "nothing was written");

    let allowed = SeedSpec {
        into_existing: true,
        ..spec(100, 1)
    };
    store.seed_synthetic(SYS, allowed).unwrap();
    assert_eq!(store.count(SYS), 101);
    // Seeding the same spec again regenerates the same epics: the
    // second batch must continue their numbering (the normal
    // allocation), which the UNIQUE (epic, seq) constraint would
    // otherwise reject.
    store.seed_synthetic(SYS, allowed).unwrap();
    assert_eq!(store.count(SYS), 201);
    let all = store.list(SYS, None, None, None).await.unwrap();
    let mut ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 201);
    assert!(store.get(SYS, "REAL-1").await.is_ok());
    // The normal write path still works afterwards.
    let next = store.create(SYS, new_ticket("REAL", "after the seed")).await.unwrap();
    assert_eq!(next.id, "REAL-2");
}

#[test]
fn seed_of_zero_is_a_no_op_and_small_counts_work() {
    let store = Store::new();
    let r = store.seed_synthetic(SYS, spec(0, 1)).unwrap();
    assert_eq!((r.count, r.epics, r.edges), (0, 0, 0));
    assert_eq!(store.count(SYS), 0);
    for n in [1, 2, 3, 10] {
        let s = Store::new();
        s.seed_synthetic(SYS, spec(n, 1)).unwrap();
        assert_eq!(s.count(SYS), n);
    }
}

#[tokio::test]
async fn concurrent_creates_never_collide_on_ids() {
    let store = Arc::new(Store::new());
    store.create(SYS, scope("RACE", "root")).await.unwrap();
    let mut handles = Vec::new();
    for i in 0..16 {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            store
                .create(SYS, new_ticket("RACE", &format!("t{i}")))
                .await
                .unwrap()
                .id
        }));
    }
    let mut ids = Vec::new();
    for h in handles {
        ids.push(h.await.unwrap());
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 16, "every concurrent create got a unique id");
}
// ---- MNG-12: every shortcode starts with a scope; set_parent --------

/// MNG-12 criterion 1: the first ticket of a prefix must be a scope —
/// anything else is refused naming the rule and nothing is allocated;
/// once the root exists, any kind follows and, naming no parent,
/// hangs under the root (MNG-14).
#[tokio::test]
async fn first_ticket_of_a_shortcode_must_be_a_scope() {
    let store = Store::new();
    let err = store.create(SYS, new_ticket("FRESH", "a build first")).await.unwrap_err();
    match &err {
        StoreError::FirstMustBeScope { epic, kind } => {
            assert_eq!(epic, "FRESH");
            assert_eq!(*kind, Kind::Build);
        }
        other => panic!("expected FirstMustBeScope, got {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("FRESH") && msg.contains("scope") && msg.contains("build"), "{msg}");
    assert_eq!(store.count(SYS), 0, "nothing was created");

    let root = store.create(SYS, scope("fresh", "the root")).await.unwrap();
    assert_eq!(root.id, "FRESH-1", "the refused create allocated nothing");
    assert_eq!(root.kind, Kind::Scope);
    let next = store.create(SYS, new_ticket("FRESH", "a build second")).await.unwrap();
    assert_eq!(next.id, "FRESH-2");
    assert_eq!(next.kind, Kind::Build);
    assert_eq!(next.parent.as_deref(), Some("FRESH-1"), "defaults to the shortcode root");
    assert_eq!(children(&store, "FRESH-1"), vec!["FRESH-2"]);
}

/// MNG-14: a parent named must be inside the ticket's own shortcode;
/// the shortcode's scope root is the exception — its parent is the
/// shortcode it hangs under, in any prefix (a sub-shortcode).
#[tokio::test]
async fn a_parent_stays_inside_the_shortcode_except_for_the_root() {
    let store = Store::new();
    store.create(SYS, scope("HOST", "the host")).await.unwrap();
    // A new shortcode's root may hang under HOST-1 …
    let sub = store
        .create(SYS, NewTicket {
            path: None,
            kind: Kind::Scope,
            ..child_of("SUB", "a sub-shortcode", "HOST-1")
        })
        .await
        .unwrap();
    assert_eq!(sub.parent.as_deref(), Some("HOST-1"));
    // … but its non-root tickets may not.
    let err = store.create(SYS, child_of("SUB", "escapes", "HOST-1")).await.unwrap_err();
    match &err {
        StoreError::ParentOutsideShortcode { id, parent } => {
            assert_eq!(id, "SUB-2");
            assert_eq!(parent, "HOST-1");
        }
        other => panic!("expected ParentOutsideShortcode, got {other:?}"),
    }
    assert_eq!(store.count(SYS), 2, "nothing was created");
    // Nor be moved out afterwards; the root can be re-hosted.
    store.create(SYS, new_ticket("SUB", "inside")).await.unwrap(); // SUB-2 → SUB-1
    assert!(matches!(
        store.set_parent(SYS.moving("SUB-2", "HOST-1")).await.unwrap_err(),
        StoreError::ParentOutsideShortcode { .. }
    ));
    store.create(SYS, scope("HOST2", "another host")).await.unwrap();
    store.set_parent(SYS.moving("SUB-1", "HOST2-1")).await.unwrap();
    assert_eq!(store.get(SYS, "SUB-1").await.unwrap().parent.as_deref(), Some("HOST2-1"));
    assert_eq!(children(&store, "HOST-1"), Vec::<String>::new());
    assert_eq!(children(&store, "HOST2-1"), vec!["SUB-1"]);
}

/// MNG-12 criterion 2: `set_parent` moves a ticket — the child, the
/// new parent and the old parent move together, the three events carry
/// the child whole and each parent's id and count (SCALE-4), the edge
/// survives a reopen, and repeating it is a no-op. (Adopting a *parentless* non-root is the MNG-12 backfill's
/// case; since MNG-14 none is born that way.)
#[tokio::test]
async fn set_parent_moves_a_ticket_broadcasts_and_is_durable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("move.db");
    let store = Store::open(&path).unwrap();
    store.create(SYS, scope("AD", "the root")).await.unwrap(); // AD-1
    store.create(SYS, new_ticket("AD", "a")).await.unwrap(); // AD-2 → AD-1
    store.create(SYS, new_ticket("AD", "b")).await.unwrap(); // AD-3 → AD-1
    assert_eq!(children(&store, "AD-1"), vec!["AD-2", "AD-3"]);
    let mut rx = store.subscribe();

    match store.set_parent(SYS.moving("ad-3", "ad-2")).await.unwrap() {
        Change::ParentChanged { ticket, previous } => {
            assert_eq!(ticket.id, "AD-3");
            assert_eq!(ticket.parent.as_deref(), Some("AD-2"));
            assert_eq!(previous.as_deref(), Some("AD-1"));
        }
        other => panic!("expected ParentChanged, got {other:?}"),
    }
    match rx.change().await.unwrap() {
        Change::ParentChanged { ticket, .. } => {
            assert_eq!(ticket.parent.as_deref(), Some("AD-2"))
        }
        other => panic!("expected ParentChanged, got {other:?}"),
    }
    match rx.change().await.unwrap() {
        Change::ChildrenChanged { id, added, child_count, .. } => {
            assert_eq!(id, "AD-2");
            assert_eq!(added, "AD-3");
            assert_eq!(child_count, 1, "the parent's id and count, not the parent");
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match rx.change().await.unwrap() {
        Change::ChildRemoved { id, removed, child_count, .. } => {
            assert_eq!(id, "AD-1");
            assert_eq!(removed, "AD-3");
            assert_eq!(child_count, 1, "the ex-parent's id and count");
        }
        other => panic!("expected ChildRemoved, got {other:?}"),
    }
    assert!(rx.try_change().is_err());

    // Idempotent: the parent it already has writes and emits nothing.
    match store.set_parent(SYS.moving("AD-3", "AD-2")).await.unwrap() {
        Change::ParentChanged { previous, .. } => assert_eq!(previous.as_deref(), Some("AD-2")),
        other => panic!("expected ParentChanged, got {other:?}"),
    }
    assert!(rx.try_change().is_err(), "a no-op emits nothing");

    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(SYS, "AD-3").await.unwrap().parent.as_deref(), Some("AD-2"));
    assert_eq!(children(&store, "AD-2"), vec!["AD-3"]);
    assert_eq!(children(&store, "AD-1"), vec!["AD-2"]);
}

/// MNG-12 criterion 3: the guards, and a move. A missing child, a
/// parent that names no ticket, and any chain that comes back around
/// (itself included) are refused with nothing changed; moving a child
/// removes it from the old parent and says so.
#[tokio::test]
async fn set_parent_guards_ancestry_and_a_move_removes_from_the_old_parent() {
    let store = Store::new();
    store.create(SYS, scope("MV", "root")).await.unwrap(); // MV-1
    store.create(SYS, child_of("MV", "a", "MV-1")).await.unwrap(); // MV-2
    store.create(SYS, child_of("MV", "b", "MV-2")).await.unwrap(); // MV-3
    store.create(SYS, new_ticket("MV", "loose")).await.unwrap(); // MV-4 → MV-1 (MNG-14)

    assert!(matches!(
        store.set_parent(SYS.moving("MV-9", "MV-1")).await.unwrap_err(),
        StoreError::NotFound(_)
    ));
    match store.set_parent(SYS.moving("MV-4", "MV-9")).await.unwrap_err() {
        StoreError::ParentNotFound { id, parent } => {
            assert_eq!(id, "MV-4");
            assert_eq!(parent, "MV-9");
        }
        other => panic!("expected ParentNotFound, got {other:?}"),
    }
    match store.set_parent(SYS.moving("MV-2", "MV-2")).await.unwrap_err() {
        StoreError::ParentCycle { id, path } => {
            assert_eq!(id, "MV-2");
            assert_eq!(path, vec!["MV-2"]);
        }
        other => panic!("expected ParentCycle, got {other:?}"),
    }
    match store.set_parent(SYS.moving("MV-1", "MV-3")).await.unwrap_err() {
        StoreError::ParentCycle { id, path } => {
            assert_eq!(id, "MV-1");
            assert_eq!(path, vec!["MV-3", "MV-2", "MV-1"]);
            let msg = StoreError::ParentCycle { id, path }.to_string();
            assert!(msg.contains("own ancestor") && msg.contains("MV-3 -> MV-2 -> MV-1"), "{msg}");
        }
        other => panic!("expected ParentCycle, got {other:?}"),
    }
    assert_eq!(store.get(SYS, "MV-1").await.unwrap().parent, None, "nothing changed");
    assert_eq!(children(&store, "MV-2"), vec!["MV-3"]);

    // The move: MV-3 from under MV-2 to under MV-1.
    let mut rx = store.subscribe();
    store.set_parent(SYS.moving("MV-3", "MV-1")).await.unwrap();
    assert!(matches!(
        rx.change().await.unwrap(),
        Change::ParentChanged { previous: Some(p), .. } if p == "MV-2"
    ));
    match rx.change().await.unwrap() {
        Change::ChildrenChanged { id, added, child_count, .. } => {
            assert_eq!((id.as_str(), added.as_str(), child_count), ("MV-1", "MV-3", 3));
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match rx.change().await.unwrap() {
        Change::ChildRemoved { id, removed, child_count, .. } => {
            assert_eq!((id.as_str(), removed.as_str(), child_count), ("MV-2", "MV-3", 0));
        }
        other => panic!("expected ChildRemoved, got {other:?}"),
    }
    assert_eq!(store.get(SYS, "MV-3").await.unwrap().parent.as_deref(), Some("MV-1"));
    // SCALE-4: in the store's order, not the order they arrived in.
    assert_eq!(children(&store, "MV-1"), vec!["MV-2", "MV-3", "MV-4"]);
    assert_eq!(children(&store, "MV-2"), Vec::<String>::new());
}

// ---- MNG-10: the roll-ups are the nav's plan() -----------------------

use crate::nav_plan::{self, Facts};
use std::collections::BTreeSet;

/// What the nav draws for this store's rows: AGS-20's `plan()` over
/// every ticket, as groups.
pub(super) fn nav(store: &Store) -> Vec<Group> {
    let all = store.page(SYS, PageQuery::default()).unwrap().tickets;
    let facts: Vec<Facts> = all
        .iter()
        .map(|t| Facts {
            id: t.id.to_string(),
            epic: t.epic.to_string(),
            seq: t.seq,
            kind: t.kind.to_string(),
            parent: t.parent.clone().map(String::from),
        })
        .collect();
    let complete: BTreeSet<String> = all
        .iter()
        .filter(|t| t.status == Status::Complete)
        .map(|t| t.id.to_string())
        .collect();
    nav_plan::rows(&facts, &complete)
        .into_iter()
        .map(|r| Group {
            epic: r.epic.as_str().into(),
            shortcode: r.epic.as_str().into(),
            // ORG-3: the registry's — held to the rule in tests_shortcodes.
            address: store.shortcode_named_by(&ShortcodeId::new(&r.epic)).1,
            total: r.total,
            done: r.done,
            docs: r.docs,
            archived: 0,
            // AGS-23: the root's own row, as the rows have it — so a write
            // that changes it is a group that moved, here as on the wire.
            root_row: r.root.as_ref().and_then(|id| all.iter().find(|t| t.id == *id)).map(|t| RootRow {
                title: t.title.clone(),
                status: t.status,
                kind: t.kind,
            }),
            root: r.root.map(TicketId::new),
            host: r.host.map(TicketId::new),
        })
        .collect()
}

/// ORG-3: a row as the file holds it — no address (the registry gives one),
/// its letters its shortcode's id.
pub(super) fn undressed(r: &Row) -> Row {
    Row { address: String::new(), epic: crate::address::Code::stored(r.shortcode.as_str()), ..r.clone() }
}

/// AGS-23: groups as the literal fixtures below spell them — counts, root
/// and host; the root's row is held to the oracle (`nav`) instead.
///
/// ORG-3: and without their full addresses, which the registry gives.
fn bare(groups: Vec<Group>) -> Vec<Group> {
    groups.into_iter().map(|g| Group { root_row: None, address: String::new(), ..g }).collect()
}

/// Every `GroupChanged` waiting on `rx`, in the order sent (the ticket
/// changes around them drained and dropped).
fn group_changes(rx: &mut broadcast::Receiver<Change>) -> Vec<Group> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_change() {
        if let Change::GroupChanged { group } = change {
            out.push(group);
        }
    }
    out
}

/// What a write must report, given the nav's drawing before and after
/// it: every row that changed or appeared, and a row no longer drawn
/// as `total: 0` — in shortcode order.
fn moved(before: &[Group], after: &[Group]) -> Vec<Group> {
    let mut out: Vec<Group> = after.iter().filter(|g| !before.contains(g)).cloned().collect();
    for g in before {
        if !after.iter().any(|a| a.epic == g.epic) {
            out.push(Group {
                total: 0,
                done: 0,
                docs: 0,
                host: None,
                ..g.clone()
            });
        }
    }
    out.sort_by(|a, b| a.epic.as_str().cmp(b.epic.as_str()));
    out
}

fn group(epic: &str, total: u64, done: u64, root: &str, host: Option<&str>) -> Group {
    Group {
        epic: epic.into(),
        shortcode: epic.into(),
        address: String::new(),
        total,
        done,
        docs: 0,
        archived: 0,
        root: Some(root.into()),
        root_row: None,
        host: host.map(Into::into),
    }
}

/// The oracle is the nav's function: the fixtures `nav.rs` pins its
/// forest with draw the same subtrees here.
#[test]
fn the_oracle_draws_the_navs_own_fixtures() {
    let f = |id: &str, kind: &str, parent: Option<&str>| {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        Facts {
            id: id.into(),
            epic: epic.into(),
            seq: seq.parse().unwrap(),
            kind: kind.into(),
            parent: parent.map(Into::into),
        }
    };
    let p = nav_plan::plan(&[
        f("AGS-1", "scope", None),
        f("AGS-19", "build", None),
        f("PERF-1", "scope", None),
        f("PERF-3", "build", None),
        f("PERF-4", "execution", Some("PERF-3")),
        f("PERF-5", "gap", Some("PERF-4")),
        f("XPC-1", "build", Some("AGS-19")),
    ]);
    assert_eq!(p.top, vec!["AGS", "PERF"], "XPC has no parentless ticket");
    assert_eq!(p.subtree["AGS"], vec!["AGS-1", "AGS-19", "XPC-1"]);
    assert_eq!(p.subtree["PERF"], vec!["PERF-1", "PERF-3", "PERF-4", "PERF-5"]);
    let p = nav_plan::plan(&[
        f("PLEXUS-1", "scope", None),
        f("PLEXUS-2", "build", Some("PLEXUS-1")),
        f("TRANSPORT-1", "scope", Some("PLEXUS-1")),
        f("TRANSPORT-2", "build", Some("TRANSPORT-1")),
    ]);
    assert_eq!(p.top, vec!["PLEXUS"], "TRANSPORT is never top-level");
    assert_eq!(p.sub_of.get("TRANSPORT").map(String::as_str), Some("PLEXUS-1"));
    assert_eq!(
        p.subtree["PLEXUS"],
        vec!["PLEXUS-1", "PLEXUS-2", "TRANSPORT-1", "TRANSPORT-2"]
    );
    assert_eq!(p.subtree["TRANSPORT"], vec!["TRANSPORT-1", "TRANSPORT-2"]);
    let p = nav_plan::plan(&[
        f("CYC-1", "build", Some("CYC-2")),
        f("CYC-2", "build", Some("CYC-1")),
        f("CYC-3", "build", Some("CYC-1")),
    ]);
    assert_eq!(p.top, vec!["CYC"]);
    assert_eq!(p.subtree["CYC"], vec!["CYC-1", "CYC-2", "CYC-3"]);
    let p = nav_plan::plan(&[
        f("AAA-1", "scope", Some("BBB-2")),
        f("BBB-1", "scope", Some("AAA-1")),
        f("BBB-2", "build", None),
    ]);
    assert!(p.sub_of.len() <= 1, "{:?}", p.sub_of);
    let reachable: usize = p.top.iter().map(|g| p.subtree[g].len()).sum();
    assert_eq!(reachable, 3, "every ticket is drawn somewhere");
}

/// MNG-10 acceptance 1 and 2 at the store level, on the shape the nav
/// rule exists for: TRAN hosted under PLEX-1 and WIRE under TRAN-2 — a
/// sub-shortcode of a sub-shortcode. Each row counts its subtree; a
/// completion moves `done` in the row it is drawn in and in every row
/// around it, innermost first, after the ticket's own event; a status
/// change that does not cross `Complete` moves nothing; births and
/// re-hosting move totals; equal to the nav's `plan()` throughout.
#[tokio::test]
async fn groups_roll_up_sub_shortcodes_as_the_nav_draws_them() {
    let store = Store::new();
    let sub = |epic: &str, title: &str, host: &str| NewTicket {
        path: None,
        kind: Kind::Scope,
        ..child_of(epic, title, host)
    };
    store.create(SYS, scope("PLEX", "plexus")).await.unwrap(); // PLEX-1
    store.create(SYS, new_ticket("PLEX", "core")).await.unwrap(); // PLEX-2 → PLEX-1
    store.create(SYS, sub("TRAN", "transport", "PLEX-1")).await.unwrap(); // TRAN-1
    store.create(SYS, new_ticket("TRAN", "ws")).await.unwrap(); // TRAN-2 → TRAN-1
    store.create(SYS, new_ticket("TRAN", "rest")).await.unwrap(); // TRAN-3 → TRAN-1
    store.create(SYS, sub("WIRE", "wire", "TRAN-2")).await.unwrap(); // WIRE-1
    store.create(SYS, new_ticket("WIRE", "frames")).await.unwrap(); // WIRE-2 → WIRE-1
    store.create(SYS, scope("SOLO", "alone")).await.unwrap(); // SOLO-1
    store.create(SYS, new_ticket("SOLO", "one")).await.unwrap(); // SOLO-2 → SOLO-1

    assert_eq!(
        bare(store.groups(SYS)),
        vec![
            group("PLEX", 7, 0, "PLEX-1", None),
            group("SOLO", 2, 0, "SOLO-1", None),
            group("TRAN", 5, 0, "TRAN-1", Some("PLEX-1")),
            group("WIRE", 2, 0, "WIRE-1", Some("TRAN-2")),
        ]
    );
    assert_eq!(store.groups(SYS), nav(&store));
    let mut rx = store.subscribe();

    // Acceptance 2: one shortcode, one event, done + 1, after the
    // status change it follows from.
    store.set_status(SYS.permit("SOLO-2"), Status::Complete).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::StatusChanged { .. }));
    match rx.try_change().unwrap() {
        Change::GroupChanged { group: g } => assert_eq!(bare(vec![g]), [group("SOLO", 2, 1, "SOLO-1", None)]),
        other => panic!("expected GroupChanged, got {other:?}"),
    }
    // WAVE-6: and, last, the placement the completion moved.
    assert!(matches!(rx.try_change().unwrap(), Change::PlacementsChanged { .. }));
    assert!(rx.try_change().is_err(), "one group moved, one event");

    // Not a crossing of Complete: nothing moves.
    store.set_status(SYS.permit("WIRE-2"), Status::Ready).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::StatusChanged { .. }));
    assert!(rx.try_change().is_err(), "Pending → Ready moves no count");

    // Deepest first: WIRE, the TRAN row it nests in, then PLEX.
    store.set_status(SYS.permit("WIRE-2"), Status::Complete).await.unwrap();
    assert_eq!(
        bare(group_changes(&mut rx)),
        vec![
            group("WIRE", 2, 1, "WIRE-1", Some("TRAN-2")),
            group("TRAN", 5, 1, "TRAN-1", Some("PLEX-1")),
            group("PLEX", 7, 1, "PLEX-1", None),
        ]
    );
    // Superseded is not done (the nav's rule): back out the same way.
    store.set_status(SYS.permit("WIRE-2"), Status::Superseded).await.unwrap();
    let done: Vec<(String, u64)> =
        group_changes(&mut rx).into_iter().map(|g| (g.epic.to_string(), g.done)).collect();
    assert_eq!(done, [("WIRE".into(), 0), ("TRAN".into(), 0), ("PLEX".into(), 0)]);

    // A birth deep inside counts in every row around it.
    store.create(SYS, new_ticket("WIRE", "more")).await.unwrap(); // WIRE-3
    let totals: Vec<(String, u64)> =
        group_changes(&mut rx).into_iter().map(|g| (g.epic.to_string(), g.total)).collect();
    assert_eq!(totals, [("WIRE".into(), 3), ("TRAN".into(), 6), ("PLEX".into(), 8)]);
    assert_eq!(store.groups(SYS), nav(&store));

    // Re-hosting a sub-shortcode moves its whole count: WIRE from under
    // TRAN-2 to under SOLO-1. After the ticket events, every row that
    // moved — innermost first, then shortcode order.
    store.set_parent(SYS.moving("WIRE-1", "SOLO-1")).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::ParentChanged { .. }));
    assert!(matches!(rx.try_change().unwrap(), Change::ChildrenChanged { .. }));
    assert!(matches!(rx.try_change().unwrap(), Change::ChildRemoved { .. }));
    assert_eq!(
        bare(group_changes(&mut rx)),
        vec![
            group("TRAN", 3, 0, "TRAN-1", Some("PLEX-1")),
            group("WIRE", 3, 0, "WIRE-1", Some("SOLO-1")),
            group("PLEX", 5, 0, "PLEX-1", None),
            group("SOLO", 5, 1, "SOLO-1", None),
        ]
    );
    assert_eq!(store.groups(SYS), nav(&store));

    // A top-level shortcode becomes a sub-shortcode: SOLO (and WIRE
    // inside it) under PLEX-2. Its pool moves whole.
    store.set_parent(SYS.moving("SOLO-1", "PLEX-2")).await.unwrap();
    assert_eq!(
        bare(store.groups(SYS)),
        vec![
            group("PLEX", 10, 1, "PLEX-1", None),
            group("SOLO", 5, 1, "SOLO-1", Some("PLEX-2")),
            group("TRAN", 3, 0, "TRAN-1", Some("PLEX-1")),
            group("WIRE", 3, 0, "WIRE-1", Some("SOLO-1")),
        ]
    );
    assert_eq!(store.groups(SYS), nav(&store));
    let epics: Vec<String> = group_changes(&mut rx).into_iter().map(|g| g.epic.to_string()).collect();
    // ORG-3: WIRE's counts did not move, its address did — PLEX/SOLO/WIRE.
    assert_eq!(epics, ["SOLO", "PLEX", "WIRE"], "WIRE's row moved by its address alone");
}

/// AGS-23: the nav draws a shortcode as its root's row, from `groups {}`
/// alone — so a group carries the root's title, status and kind, a
/// rootless group carries none, and a change to any of them is a change to
/// the group: one `GroupChanged`, the root's own shortcode's, after the
/// ticket event — for a status change that crosses no `Complete` and for a
/// title. A change to a ticket that is not a root, a body, or a status set
/// to the one it had moves no group.
#[tokio::test]
async fn a_group_carries_its_roots_row_and_a_change_to_it_is_a_group_change() {
    let store = Store::new();
    store.create(SYS, scope("HOST", "the host")).await.unwrap(); // HOST-1
    store.create(SYS, new_ticket("HOST", "work")).await.unwrap(); // HOST-2 → HOST-1
    store
        .create(SYS, NewTicket { kind: Kind::Scope, ..child_of("SUB", "the sub", "HOST-2") })
        .await
        .unwrap(); // SUB-1, hosted under HOST-2
    let row = |title: &str, status| Some(RootRow { title: title.into(), status, kind: Kind::Scope });
    let by = |epic: &str| store.groups(SYS).into_iter().find(|g| g.epic == epic).unwrap();
    assert_eq!(by("HOST").root_row, row("the host", Status::Pending));
    assert_eq!(by("SUB").root_row, row("the sub", Status::Pending));
    assert_eq!(store.groups(SYS), nav(&store));
    let mut rx = store.subscribe();

    // A root's status, crossing nothing: its group, once, after the ticket.
    store.set_status(SYS.permit("SUB-1"), Status::Ready).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::StatusChanged { .. }));
    match rx.try_change().unwrap() {
        Change::GroupChanged { group } => {
            assert_eq!((group.epic.as_str(), group.total, group.done), ("SUB", 1, 0));
            assert_eq!(group.root_row, row("the sub", Status::Ready));
        }
        other => panic!("expected GroupChanged, got {other:?}"),
    }
    assert!(rx.try_change().is_err(), "the root's group only — not the host's");

    // Across Complete: the counts' own report carries the new row, once.
    store.set_status(SYS.permit("SUB-1"), Status::Complete).await.unwrap();
    let moved = group_changes(&mut rx);
    assert_eq!(moved.iter().map(|g| g.epic.as_str()).collect::<Vec<_>>(), ["SUB", "HOST"], "innermost first");
    assert_eq!(moved[0].root_row, row("the sub", Status::Complete));
    assert_eq!(moved[1].root_row, row("the host", Status::Pending));

    // The status it already has: the ticket event, no group.
    store.set_status(SYS.permit("SUB-1"), Status::Complete).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::StatusChanged { .. }));
    assert!(rx.try_change().is_err());

    // A root's title: its group, after the `Amended`; a body alone, none.
    store.amend(SYS.permit("HOST-1"), Some("the host, renamed".into()), None).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::Amended { .. }));
    match rx.try_change().unwrap() {
        Change::GroupChanged { group } => assert_eq!(group.root_row, row("the host, renamed", Status::Pending)),
        other => panic!("expected GroupChanged, got {other:?}"),
    }
    assert!(rx.try_change().is_err());
    store.amend(SYS.permit("HOST-1"), None, Some("a body".into())).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::Amended { .. }));
    assert!(rx.try_change().is_err(), "a body is not on the row");

    // Not a root: a ticket event and nothing more.
    store.set_status(SYS.permit("HOST-2"), Status::Ready).await.unwrap();
    store.amend(SYS.permit("HOST-2"), Some("other work".into()), None).await.unwrap();
    assert!(matches!(rx.try_change().unwrap(), Change::StatusChanged { .. }));
    assert!(matches!(rx.try_change().unwrap(), Change::Amended { .. }));
    assert!(rx.try_change().is_err());
    assert_eq!(store.groups(SYS), nav(&store));
}

/// A database the server could not write today but must still load,
/// written straight through the schema.
pub(super) fn write_db(path: &Path, rows: &[(&str, Kind, Status, Option<&str>)]) {
    let tickets: Vec<Ticket> = rows
        .iter()
        .map(|(id, kind, status, parent)| {
            let (epic, seq) = id.rsplit_once('-').unwrap();
            Ticket {
                hidden_blocked_by: 0,
                hidden_unlocks: 0,
                parent_hidden: false,
                archived: None,
                labels: Vec::new(),
                id: (*id).into(),
                epic: epic.into(),
                shortcode: epic.into(),
                address: String::new(),
                seq: seq.parse().unwrap(),
                title: id.to_string(),
                status: *status,
                kind: *kind,
                severity: None,
                blocked_by: Vec::new(),
                unlocks: Vec::new(),
                parent: parent.map(Into::into),
                child_count: 0,
                commits: Vec::new(),
                path: None,
                placement: None,
                body: String::new(),
                created_unix: 0,
                updated_unix: 0,
                moved_from: Vec::new(),
            }
        })
        .collect();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(db::SCHEMA).unwrap();
    for t in &tickets {
        db::upsert(&conn, t).unwrap();
    }
}

/// Every shape the nav has a rule for, most of which the server no
/// longer makes: parentless non-roots, a root that is not seq 1, a
/// prefix with no scope, a non-root filed under the host, a
/// cross-shortcode child with no row of its own, two sub-shortcodes
/// that would nest inside each other — and two only a hand edit makes:
/// parent cycles (inside a shortcode, and across three and across two:
/// lengths no bounded walk lands back on by luck) and a parent that
/// names no ticket.
pub(super) fn legacy() -> Vec<(&'static str, Kind, Status, Option<&'static str>)> {
    use Kind::{Build, Scope};
    use Status::{Complete, Pending};
    vec![
        ("AAA-1", Scope, Pending, Some("BBB-2")), // plan() keeps AAA under BBB-2 …
        ("AGS-1", Scope, Complete, None),
        ("AGS-2", Build, Complete, Some("AGS-1")),
        ("AGS-3", Build, Pending, None), // a parentless non-root (pre-MNG-14)
        ("AGS-4", Build, Complete, Some("AGS-3")),
        ("BBB-1", Scope, Pending, Some("AAA-1")), // … and refuses BBB under AAA-1
        ("BBB-2", Build, Complete, None),
        ("CXA-1", Scope, Pending, Some("CXB-1")), // a cycle across three shortcodes
        ("CXB-1", Scope, Complete, Some("CXC-1")),
        ("CXB-2", Build, Complete, Some("CXB-1")),
        ("CXC-1", Scope, Pending, Some("CXA-1")),
        ("DUA-1", Scope, Pending, Some("DUB-1")), // and across two
        ("DUB-1", Scope, Complete, Some("DUA-1")),
        ("CYC-1", Build, Complete, Some("CYC-2")), // a cycle inside one
        ("CYC-2", Build, Pending, Some("CYC-1")),
        ("CYC-3", Build, Complete, Some("CYC-1")),
        ("CYC-4", Scope, Pending, None),
        ("DEMO-1", Build, Complete, None), // builds before the first scope (pre-MNG-12)
        ("DEMO-2", Build, Pending, None),
        ("DEMO-3", Build, Pending, Some("DEMO-2")),
        ("DEMO-4", Scope, Pending, None), // the root, not seq 1
        ("DEMO-5", Build, Complete, Some("DEMO-4")),
        ("NOSC-1", Build, Pending, None), // a prefix that never had a scope
        ("NOSC-2", Build, Complete, None),
        ("ORPH-1", Scope, Pending, None),
        ("ORPH-2", Build, Complete, Some("GHOST-1")), // names no ticket (yet)
        ("PLEX-1", Scope, Pending, None),
        ("PLEX-2", Build, Complete, Some("PLEX-1")),
        ("TRAN-1", Scope, Pending, Some("PLEX-1")), // a sub-shortcode
        ("TRAN-2", Build, Complete, Some("TRAN-1")),
        ("TRAN-3", Build, Pending, Some("PLEX-1")), // a non-root under the host: TRAN's row
        ("XPC-1", Build, Complete, Some("AGS-3")), // a cross child with no scope: no row
        ("XPC-2", Build, Pending, Some("XPC-1")),
    ]
}

/// Loading draws what plan() draws, every legacy and hand-edited shape
/// included; and the two writes that repair a hand edit — a `set_parent`
/// that moves a ticket off a cycle, a create of the id a dangling
/// parent names — leave it drawn as plan() draws the result, reporting
/// exactly the rows that moved.
#[tokio::test]
async fn hand_edited_shapes_roll_up_as_drawn_and_their_repair_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    write_db(&path, &legacy());
    let store = Store::open(&path).unwrap();
    let drawn = store.groups(SYS);
    assert_eq!(drawn, nav(&store));
    let by = |epic: &str| drawn.iter().find(|g| g.epic == epic).cloned();
    assert_eq!(by("XPC"), None, "XPC's tickets are all drawn under AGS-3");
    assert_eq!(by("AGS").map(|g| (g.total, g.done)), Some((6, 4)), "XPC counts in AGS");
    assert_eq!(by("AAA").and_then(|g| g.host).as_deref(), Some("BBB-2"));
    assert_eq!(by("BBB").and_then(|g| g.host), None, "the second nesting is refused");
    assert_eq!(by("DEMO").and_then(|g| g.root).as_deref(), Some("DEMO-4"));
    assert_eq!(by("NOSC").map(|g| g.root), Some(None), "never had a scope");
    assert_eq!(by("TRAN").map(|g| g.total), Some(3), "TRAN-3 is drawn in TRAN's row");
    assert_eq!(by("CYC").map(|g| (g.total, g.done)), Some((4, 2)));
    assert_eq!(by("ORPH").map(|g| g.total), Some(2), "a dangling parent is no parent");
    for (epic, total) in [("CXA", 1), ("CXB", 2), ("CXC", 1), ("DUA", 1), ("DUB", 1)] {
        let g = by(epic).unwrap();
        assert_eq!((g.total, g.host), (total, None), "{epic}: a cycle member is a root");
    }
    let mut rx = store.subscribe();

    // Off the cycle: CYC-2 under CYC-4 frees CYC-1 (and CYC-3 with
    // it) — all still in CYC's row, so nothing is reported.
    store.set_parent(SYS.moving("CYC-2", "CYC-4")).await.unwrap();
    assert_eq!(store.groups(SYS), nav(&store));
    assert_eq!(group_changes(&mut rx), Vec::<Group>::new());

    // Off the cross cycle: CXA's root under PLEX-2 frees the other two
    // — CXC-1's parent CXA-1 makes CXC a sub-shortcode of CXA, and
    // CXB-1's parent CXC-1 one of CXC: three rows nest into PLEX at
    // once, each inside the last.
    let before = nav(&store);
    store.set_parent(SYS.moving("CXA-1", "PLEX-2")).await.unwrap();
    let after = nav(&store);
    assert_eq!(store.groups(SYS), after);
    let at = |epic: &str| after.iter().find(|g| g.epic == epic).cloned().unwrap();
    assert_eq!((at("CXA").total, at("CXA").host.as_deref()), (4, Some("PLEX-2")));
    assert_eq!((at("CXC").total, at("CXC").host.as_deref()), (3, Some("CXA-1")));
    assert_eq!((at("CXB").total, at("CXB").host.as_deref()), (2, Some("CXC-1")));
    assert_eq!((at("PLEX").total, at("PLEX").done), (9, 4));
    let got = group_changes(&mut rx);
    assert_eq!(
        got.iter().map(|g| g.epic.as_str()).collect::<Vec<_>>(),
        ["CXB", "CXC", "CXA", "PLEX"],
        "innermost first"
    );
    let mut got = got;
    got.sort_by(|a, b| a.epic.as_str().cmp(b.epic.as_str()));
    assert_eq!(got, moved(&before, &after));

    // The ghost is born: ORPH-2 now hangs under it, in GHOST's row.
    let before = nav(&store);
    store.create(SYS, scope("GHOST", "late")).await.unwrap();
    let after = nav(&store);
    assert_eq!(store.groups(SYS), after);
    assert_eq!(
        after.iter().find(|g| g.epic == "GHOST").map(|g| (g.total, g.done)),
        Some((2, 1))
    );
    let mut got = group_changes(&mut rx);
    got.sort_by(|a, b| a.epic.as_str().cmp(b.epic.as_str()));
    assert_eq!(got, moved(&before, &after));
    assert_eq!(got.iter().map(|g| g.epic.as_str()).collect::<Vec<_>>(), ["GHOST", "ORPH"]);
}

/// MNG-10 acceptance 1, the long way: from the legacy database, 800
/// pseudo-random writes — creates (new shortcodes, sub-shortcodes,
/// refused ones), status changes and `set_parent` moves of tickets and
/// of roots across shortcodes — and after every one, the roll-ups are
/// exactly what the nav's `plan()` draws from all rows, and the
/// `GroupChanged` events are exactly the rows that moved: no more, no
/// fewer, each once. A reopen then rebuilds the same roll-ups.
#[tokio::test]
async fn the_roll_ups_stay_the_navs_plan_through_every_kind_of_write() {
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) % n as u64) as usize
        }
    }
    const EPICS: [&str; 13] = [
        "AAA", "AGS", "BBB", "CYC", "DEMO", "NOSC", "PLEX", "TRAN", "XPC", "GHOST", "NEW",
        "SUBB", "ZED",
    ];

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("walk.db");
    write_db(&path, &legacy());
    let store = Store::open(&path).unwrap();
    assert_eq!(store.groups(SYS), nav(&store));
    let mut rx = store.subscribe();
    let mut rng = Rng(0x4D4E_4731);
    let (mut writes, mut moves, mut rehosts) = (0, 0, 0);

    for step in 0..800 {
        let all = store.list(SYS, None, None, None).await.unwrap();
        let before = nav(&store);
        let any = |rng: &mut Rng| all[rng.below(all.len())].id.clone();
        let what = match rng.below(10) {
            0..=3 => {
                let epic = EPICS[rng.below(EPICS.len())];
                // SDLC-11: docs too — roots of new shortcodes, children of
                // work, parents of work — counted in `docs`, never `total`.
                let kind = match rng.below(6) {
                    0 | 1 => Kind::Scope,
                    2 => Kind::Doc,
                    _ => Kind::Build,
                };
                let mine: Vec<&Ticket> = all.iter().filter(|t| t.epic == epic).collect();
                let parent = match rng.below(5) {
                    0 | 1 => None,
                    2 | 3 if !mine.is_empty() => Some(mine[rng.below(mine.len())].id.clone()),
                    _ => Some(any(&mut rng)),
                };
                let parent = super::tests_ready::no_namesake(&store, epic, kind, parent.map(String::from));
                let what = format!("create {epic} {kind} under {parent:?}");
                let new = NewTicket {
                    path: None,
                    kind,
                    parent,
                    ..new_ticket(epic, "walked")
                };
                if store.create(SYS, new).await.is_ok() {
                    writes += 1;
                }
                what
            }
            4..=7 => {
                let id = any(&mut rng);
                let status = if rng.below(2) == 0 {
                    Status::Complete
                } else {
                    Status::ALL[rng.below(Status::ALL.len())]
                };
                // SDLC-11: a doc's status never moves, and Doc is no other
                // kind's — both refused, and nothing is drawn differently.
                let kind = store.get(SYS, &id).await.unwrap().kind;
                match store.set_status(SYS.permit(&id), status).await {
                    Ok(_) => writes += 1,
                    Err(StoreError::DocHasNoLifecycle { .. }) => assert_eq!(kind, Kind::Doc, "{id}"),
                    Err(StoreError::DocStatusOnWork { .. }) => assert_eq!(status, Status::Doc, "{id}"),
                    Err(e) => panic!("set_status {id} {status}: {e}"),
                }
                format!("set_status {id} {status}")
            }
            _ => {
                // A root a third of the time: re-hosting shortcodes is
                // the reshaping the roll-ups must follow.
                let child = if rng.below(3) == 0 {
                    before[rng.below(before.len())].root.clone().map(String::from).unwrap_or_else(|| String::from(any(&mut rng)))
                } else {
                    String::from(any(&mut rng))
                };
                let epic = store.get(SYS, &child).await.unwrap().epic;
                let mine: Vec<&Ticket> = all.iter().filter(|t| t.epic == epic).collect();
                let parent = if rng.below(2) == 0 {
                    mine[rng.below(mine.len())].id.clone()
                } else {
                    any(&mut rng)
                };
                let was = store.get(SYS, &child).await.unwrap().parent;
                if store.set_parent(SYS.moving(&child, &parent)).await.is_ok()
                    && was.as_deref() != Some(parent.as_str())
                {
                    writes += 1;
                    moves += 1;
                    if store.get(SYS, &parent).await.unwrap().epic != epic {
                        rehosts += 1;
                    }
                }
                format!("set_parent {child} under {parent}")
            }
        };
        let after = nav(&store);
        assert_eq!(store.groups(SYS), after, "step {step}: {what}");
        let mut got = group_changes(&mut rx);
        got.sort_by(|a, b| a.epic.as_str().cmp(b.epic.as_str()));
        assert_eq!(got, moved(&before, &after), "step {step}: {what}");
    }
    assert!(writes > 400 && moves > 40 && rehosts > 5, "{writes} writes, {moves} moves, {rehosts} re-hosts");
    // SDLC-11: docs were born, and are drawn — counted apart from the work.
    let docs = store.list(SYS, None, Some(Status::Doc), None).await.unwrap();
    assert!(docs.len() > 10 && docs.iter().all(|t| t.kind == Kind::Doc), "{} docs", docs.len());
    assert!(store.groups(SYS).iter().any(|g| g.docs > 0));

    let kept = store.groups(SYS);
    drop(store);
    assert_eq!(Store::open(&path).unwrap().groups(SYS), kept, "a reopen rebuilds the same");
}

/// The build at scale: a 10⁴ seed rolls up as the nav draws it — every
/// ticket in exactly one top-level row — and stays so when one of its
/// shortcodes is hosted under another and when a second seed lands in
/// the same store.
#[tokio::test]
async fn a_seeded_store_rolls_up_as_the_nav_draws_it() {
    let store = Store::new();
    let report = store.seed_synthetic(SYS, spec(10_000, 42)).unwrap();
    let groups = store.groups(SYS);
    assert_eq!(groups.len(), report.epics);
    assert_eq!(groups, nav(&store));
    let top: u64 = groups.iter().filter(|g| g.host.is_none()).map(|g| g.total).sum();
    assert_eq!(top, 10_000, "every ticket drawn in exactly one top-level row");

    let host = format!("{}-2", groups[0].epic);
    store
        .set_parent(SYS.moving(groups[1].root.as_deref().unwrap(), &host))
        .await
        .unwrap();
    assert_eq!(store.groups(SYS), nav(&store));
    assert_eq!(store.groups(SYS)[1].host.as_deref(), Some(host.as_str()));

    store
        .seed_synthetic(SYS, SeedSpec {
            into_existing: true,
            ..spec(500, 3)
        })
        .unwrap();
    assert_eq!(store.groups(SYS), nav(&store));
}

// ---- SCALE-3: create without scans -----------------------------------

/// What each index's question was answered with before SCALE-3, answered
/// the same way here — by walking a `list` of the whole store: each
/// shortcode's highest sequence, and every id some `blocked_by` names that
/// is no ticket, with the tickets naming it sorted by (epic, seq) — the
/// order the old scan sorted a newborn's `unlocks` into.
pub(super) type Scanned = (BTreeMap<String, u32>, BTreeMap<String, Vec<String>>);

pub(super) fn scanned(store: &Store) -> Scanned {
    let all = store.page(SYS, PageQuery::default()).unwrap().tickets;
    let at: HashMap<&str, (&str, u32)> =
        all.iter().map(|t| (t.id.as_str(), (t.epic.as_str(), t.seq))).collect();
    let mut last: BTreeMap<String, u32> = BTreeMap::new();
    let mut unborn: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in &all {
        let seq = last.entry(t.epic.to_string()).or_insert(0);
        *seq = (*seq).max(t.seq);
    }
    let named: BTreeSet<&str> = all
        .iter()
        .flat_map(|t| t.blocked_by.iter().map(TicketName::as_str))
        .filter(|b| !at.contains_key(b))
        .collect();
    for id in named {
        let mut waiting: Vec<&TicketSummary> =
            all.iter().filter(|t| t.blocked_by.iter().any(|b| b == id)).collect();
        waiting.sort_by_key(|t| at[t.id.as_str()]);
        unborn.insert(id.to_string(), waiting.iter().map(|t| t.id.to_string()).collect());
    }
    (last, unborn)
}

/// What the cache keeps, in the same shape.
fn kept(store: &Store) -> Scanned {
    let cache = store.tickets.read().unwrap();
    let last = cache
        .ids
        .last_seqs()
        .map(|(epic, seq)| (epic.as_str().to_string(), seq))
        .collect();
    (last, cache.ids.unborn())
}

#[track_caller]
fn assert_kept_is_a_scan(store: &Store, after: &str) {
    assert_eq!(kept(store), scanned(store), "after {after}");
}

/// Writes tickets straight through the schema — any shape, the server's
/// or not — with `unlocks` mirrored for every blocker that exists, as the
/// store keeps it. `(id, kind, blocked_by)`; no parents.
fn write_blocked_db(path: &Path, rows: &[(&str, Kind, &[&str])]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(db::SCHEMA).unwrap();
    for (id, kind, blocked_by) in rows {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        let unlocks: Vec<String> = rows
            .iter()
            .filter(|(_, _, b)| b.contains(id))
            .map(|(other, _, _)| other.to_string())
            .collect();
        db::upsert(
            &conn,
            &Ticket {
                hidden_blocked_by: 0,
                hidden_unlocks: 0,
                parent_hidden: false,
                archived: None,
                labels: Vec::new(),
                id: (*id).into(),
                epic: epic.into(),
                shortcode: epic.into(),
                address: String::new(),
                seq: seq.parse().unwrap(),
                title: id.to_string(),
                status: Status::Pending,
                kind: *kind,
                severity: None,
                blocked_by: blocked_by.iter().map(|b| (*b).into()).collect(),
                unlocks: unlocks.into_iter().map(TicketName::new).collect(),
                parent: None,
                child_count: 0,
                commits: Vec::new(),
                path: None,
                placement: None,
                body: String::new(),
                created_unix: 0,
                updated_unix: 0,
                moved_from: Vec::new(),
            },
        )
        .unwrap();
    }
}

/// Acceptance 2 and the ticket's third row: a store whose tickets name an
/// id before it exists — from three shortcodes, one of them sorting before
/// the id's own — is closed and reopened from disk, and the ticket born
/// with that id has exactly those tickets as its `unlocks`, in (epic, seq)
/// order, as the scan this replaced gave it.
#[tokio::test]
async fn a_reopened_store_resolves_forward_references_at_birth_as_the_scan_did() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forward.db");
    {
        let store = Store::open(&path).unwrap();
        store
            .create(SYS, NewTicket {
                path: None,
                blocked_by: vec!["FWD-3".into()],
                ..scope("FWD", "names its third ticket")
            })
            .await
            .unwrap();
        store
            .create(SYS, NewTicket {
                path: None,
                blocked_by: vec!["FWD-3".into(), "FWD-3".into(), "ZZZ-1".into()],
                ..new_ticket("FWD", "names it twice, and an unborn shortcode")
            })
            .await
            .unwrap();
        store
            .create(SYS, NewTicket {
                path: None,
                blocked_by: vec!["FWD-3".into()],
                ..scope("AAA", "sorts before FWD, born after it")
            })
            .await
            .unwrap();
        store
            .create(SYS, NewTicket {
                path: None,
                blocked_by: vec!["FWD-3".into()],
                ..scope("ZZZ", "sorts after")
            })
            .await
            .unwrap();
        assert_kept_is_a_scan(&store, "the creates");
    }

    let store = Store::open(&path).unwrap();
    assert_kept_is_a_scan(&store, "the reopen");
    let (_, unborn) = scanned(&store);
    assert_eq!(unborn["FWD-3"], ["AAA-1", "FWD-1", "FWD-2", "ZZZ-1"]);
    assert_eq!(store.get(SYS, "ZZZ-1").await.unwrap().unlocks, ["FWD-2"], "resolved at ZZZ-1's birth");

    let born = store.create(SYS, new_ticket("FWD", "the named one")).await.unwrap();
    assert_eq!(born.id, "FWD-3");
    assert_eq!(born.unlocks, unborn["FWD-3"], "what the scan gave");
    assert_eq!(store.get(SYS, "FWD-3").await.unwrap().unlocks, born.unlocks);
    assert_kept_is_a_scan(&store, "the birth");
    assert_eq!(kept(&store).1, BTreeMap::new(), "nothing left unborn");

    // And durably so: one more reopen reads it back from the file.
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(SYS, "FWD-3").await.unwrap().unlocks, ["AAA-1", "FWD-1", "FWD-2", "ZZZ-1"]);
    assert_kept_is_a_scan(&store, "the second reopen");
}

/// The fourth row, for every way tickets reach the cache other than
/// `create` — loading a database the server could not have written
/// (a hole in a sequence, an id that can never be born, a lowercase name,
/// a reference to a shortcode that does not exist), `set_parent`, a
/// reopen — and for the ways `create` itself can go: each rejection
/// (blocked_by cycle, missing parent, a parent outside the shortcode, a
/// first ticket that is not a scope) allocates nothing and indexes
/// nothing, so the next create gets the id the rejected one would have
/// had. After every step the kept indexes are exactly a scan's.
#[tokio::test]
async fn the_id_indexes_are_a_scan_after_a_load_every_kind_of_create_and_set_parent() {
    use Kind::{Build, Scope};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shapes.db");
    write_blocked_db(
        &path,
        &[
            ("GAP-1", Scope, &[]),
            ("GAP-3", Build, &["GAP-2"]), // a hole: GAP-2 can never be born
            ("ODD-1", Scope, &["RT-0", "odd-2", "NOPE-1"]),
            ("ODD-2", Build, &["ODD-1"]),
            ("WAIT-1", Scope, &["WAIT-2", "LATE-1"]),
        ],
    );
    let store = Store::open(&path).unwrap();
    assert_kept_is_a_scan(&store, "the load");
    let held = kept(&store);
    assert_eq!(held.0["GAP"], 3, "the highest, not the count");
    assert_eq!(
        held.1.keys().map(String::as_str).collect::<Vec<_>>(),
        ["GAP-2", "LATE-1", "NOPE-1", "RT-0", "WAIT-2", "odd-2"],
        "ODD-2 exists, so ODD-1's edge to it is no forward reference"
    );

    // Highest plus one, past the hole; GAP-2 stays unborn.
    let t = store.create(SYS, new_ticket("GAP", "after the hole")).await.unwrap();
    assert_eq!((t.id.as_str(), t.unlocks.len()), ("GAP-4", 0));
    assert_kept_is_a_scan(&store, "a create past a hole");

    // Rejections leave no trace, in the tickets or in the indexes.
    let before = kept(&store);
    let refused = [
        NewTicket {
            path: None,
            blocked_by: vec!["WAIT-2".into()],
            ..new_ticket("WAIT", "names itself") // would be WAIT-2
        },
        NewTicket {
            path: None,
            blocked_by: vec!["WAIT-1".into(), "NEVER-1".into()],
            ..new_ticket("WAIT", "closes WAIT-2 -> WAIT-1 -> WAIT-2")
        },
        NewTicket {
            path: None,
            blocked_by: vec!["NEVER-2".into()],
            parent: Some("NOWHERE-1".into()),
            ..new_ticket("WAIT", "a parent that is no ticket")
        },
        NewTicket {
            path: None,
            blocked_by: vec!["NEVER-3".into()],
            parent: Some("GAP-1".into()),
            ..new_ticket("WAIT", "a parent outside the shortcode")
        },
        NewTicket {
            path: None,
            blocked_by: vec!["NEVER-4".into()],
            ..new_ticket("FRESH", "a build first")
        },
    ];
    for new in refused {
        let title = new.title.clone();
        assert!(store.create(SYS, new).await.is_err(), "{title}");
        assert_eq!(kept(&store), before, "a refused create indexed nothing: {title}");
        assert_kept_is_a_scan(&store, &title);
    }
    let t = store.create(SYS, new_ticket("WAIT", "the id the refusals did not take")).await.unwrap();
    assert_eq!(t.id, "WAIT-2");
    assert_eq!(t.unlocks, ["WAIT-1"], "and the forward reference resolved");
    assert_kept_is_a_scan(&store, "WAIT-2's birth");

    // A first ticket of a shortcode named before it existed, as a parent
    // of nothing and a blocker of WAIT-1.
    let t = store.create(SYS, scope("LATE", "named by WAIT-1")).await.unwrap();
    assert_eq!((t.id.as_str(), t.unlocks.clone()), ("LATE-1", vec![TicketName::from("WAIT-1")]));
    assert_kept_is_a_scan(&store, "LATE-1's birth");

    // New forward references, and a create that is both blocker and parent.
    store
        .create(SYS, NewTicket {
            path: None,
            blocked_by: vec!["WAIT-5".into(), "LATE-3".into(), "WAIT-2".into()],
            ..new_ticket("WAIT", "WAIT-3")
        })
        .await
        .unwrap();
    assert_kept_is_a_scan(&store, "a create naming two unborn ids");
    store
        .create(SYS, NewTicket {
            path: None,
            blocked_by: vec!["WAIT-1".into()],
            parent: Some("WAIT-1".into()),
            ..new_ticket("WAIT", "WAIT-4")
        })
        .await
        .unwrap();
    let t = store.create(SYS, new_ticket("WAIT", "WAIT-5")).await.unwrap();
    assert_eq!(t.unlocks, ["WAIT-3"]);
    assert_kept_is_a_scan(&store, "WAIT-5's birth");

    // set_parent moves no sequence and no blocked_by.
    store.set_parent(SYS.moving("WAIT-3", "WAIT-2")).await.unwrap();
    assert_kept_is_a_scan(&store, "set_parent");
    store.set_parent(SYS.moving("LATE-1", "WAIT-5")).await.unwrap();
    assert_kept_is_a_scan(&store, "set_parent of a root across shortcodes");
    store.set_status(SYS.permit("WAIT-2"), Status::Complete).await.unwrap();
    assert_kept_is_a_scan(&store, "set_status");

    let held = kept(&store);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(kept(&store), held, "a reopen rebuilds what was kept");
    assert_kept_is_a_scan(&store, "the reopen");
    let t = store.create(SYS, new_ticket("LATE", "LATE-2")).await.unwrap();
    assert_eq!(t.id, "LATE-2");
    let t = store.create(SYS, new_ticket("LATE", "LATE-3")).await.unwrap();
    assert_eq!(t.unlocks, ["WAIT-3"]);
    assert_kept_is_a_scan(&store, "the creates after the reopen");
}

/// The fourth row for `seed_synthetic`, both ways: into an empty store,
/// and into one that already holds tickets — including one that names, as
/// a forward reference, an id the seed then generates (the seed is
/// deterministic, so a dry run says which). The seeded id exists now, so
/// it is no longer unborn; the next create in every shortcode, seeded or
/// not, is its highest plus one.
#[tokio::test]
async fn the_id_indexes_are_a_scan_after_a_seed_with_and_without_into_existing() {
    let fresh = Store::new();
    fresh.seed_synthetic(SYS, spec(3_000, 9)).unwrap();
    assert_kept_is_a_scan(&fresh, "a seed into an empty store");
    assert_eq!(kept(&fresh).1, BTreeMap::new(), "the generator names only what it made");

    let dry = Store::new();
    dry.seed_synthetic(SYS, spec(2_000, 5)).unwrap();
    let seeded = dry.list(SYS, None, None, None).await.unwrap()[0].id.clone(); // e.g. its first shortcode's root

    let store = Store::new();
    store.create(SYS, scope("REAL", "already here")).await.unwrap();
    store
        .create(SYS, NewTicket {
            path: None,
            blocked_by: vec![seeded.to_string().as_str().into(), "REAL-9".into()],
            ..new_ticket("REAL", "names an id the seed will make")
        })
        .await
        .unwrap();
    assert!(kept(&store).1.contains_key(seeded.as_str()));
    store
        .seed_synthetic(SYS, SeedSpec {
            into_existing: true,
            ..spec(2_000, 5)
        })
        .unwrap();
    assert!(store.get(SYS, &seeded).await.is_ok(), "the seed made {seeded}");
    assert_kept_is_a_scan(&store, "a seed into an existing store");
    assert!(!kept(&store).1.contains_key(seeded.as_str()), "{seeded} is born");

    // Again over itself: every seeded shortcode continues its numbering.
    store
        .seed_synthetic(SYS, SeedSpec {
            into_existing: true,
            ..spec(2_000, 5)
        })
        .unwrap();
    assert_kept_is_a_scan(&store, "a second seed over the first");
    let (last, _) = scanned(&store);
    for (epic, highest) in &last {
        let t = store.create(SYS, new_ticket(epic, "after the seeds")).await.unwrap();
        assert_eq!(t.seq, highest + 1, "{epic}");
    }
    assert_kept_is_a_scan(&store, "a create in every shortcode");
}

/// The fourth row for both in-place migrations: a pre-AGS-13 database
/// (`ticket_type`) and a pre-AGS-19 one (no `parent`), each holding
/// forward references, open to indexes a scan agrees with — and the named
/// id, born, unlocks exactly its referrers.
#[tokio::test]
async fn the_id_indexes_are_a_scan_after_both_in_place_migrations() {
    let dir = tempfile::tempdir().unwrap();

    let path = dir.path().join("pre-ags13.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(LEGACY_SCHEMA).unwrap();
        for (seq, tt, blocked_by) in [
            (1, "epic", r#"["LEG-4", "OUT-1"]"#),
            (2, "implementation", r#"["LEG-1"]"#),
            (3, "analysis", r#"["LEG-4"]"#),
        ] {
            conn.execute(
                "INSERT INTO tickets VALUES (?1, 'LEG', ?2, 'legacy', 'Pending', ?3, NULL, ?4, '[]', 'body', 1, 1)",
                rusqlite::params![format!("LEG-{seq}"), seq, tt, blocked_by],
            )
            .unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    assert_kept_is_a_scan(&store, "the AGS-13 migration");
    assert_eq!(kept(&store).1["LEG-4"], ["LEG-1", "LEG-3"]);
    let t = store.create(SYS, new_ticket("LEG", "the named one")).await.unwrap();
    assert_eq!((t.id.as_str(), t.unlocks.clone()), ("LEG-4", vec!["LEG-1".into(), "LEG-3".into()]));
    assert_kept_is_a_scan(&store, "LEG-4's birth");

    let path = dir.path().join("pre-ags19.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(PRE_AGS19_SCHEMA).unwrap();
        for (seq, kind, blocked_by, body) in [
            (1, "scope", r#"["OLD-3"]"#, "no frontmatter"),
            (2, "build", r#"["OLD-3", "OTHER-2"]"#, "---\nparent: OLD-1\n---\n\nbody"),
        ] {
            conn.execute(
                "INSERT INTO tickets VALUES (?1, 'OLD', ?2, 'old', 'Pending', ?3, NULL, ?4, '[]', ?5, 1, 1)",
                rusqlite::params![format!("OLD-{seq}"), seq, kind, blocked_by, body],
            )
            .unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(SYS, "OLD-2").await.unwrap().parent.as_deref(), Some("OLD-1"), "migrated");
    assert_kept_is_a_scan(&store, "the AGS-19 migration");
    let t = store.create(SYS, new_ticket("OLD", "the named one")).await.unwrap();
    assert_eq!((t.id.as_str(), t.unlocks.clone()), ("OLD-3", vec!["OLD-1".into(), "OLD-2".into()]));
    assert_kept_is_a_scan(&store, "OLD-3's birth");
}

/// Acceptance 3: after a 10⁴ seed into a store that already holds tickets
/// — one of its shortcodes the seed's own, so the seed continues it — the
/// next create in a seeded shortcode is its highest sequence plus one.
#[tokio::test]
async fn after_a_seed_into_an_existing_store_the_next_create_is_highest_plus_one() {
    let dry = Store::new();
    dry.seed_synthetic(SYS, spec(10_000, 1)).unwrap();
    let biggest = dry.groups(SYS).into_iter().max_by_key(|g| g.total).unwrap().epic;

    let store = Store::new();
    store.create(SYS, scope(&biggest, "here before the seed")).await.unwrap();
    store.create(SYS, new_ticket(&biggest, "and its second")).await.unwrap();
    store.create(SYS, scope("REAL", "a shortcode the seed does not know")).await.unwrap();
    store
        .seed_synthetic(SYS, SeedSpec {
            into_existing: true,
            ..spec(10_000, 1)
        })
        .unwrap();
    assert_eq!(store.count(SYS), 10_003);

    let highest = store.list(SYS, Some(&biggest), None, None).await.unwrap().last().unwrap().seq;
    assert!(highest > 2, "the seed continued {biggest} after the two already there");
    let t = store.create(SYS, new_ticket(&biggest, "the next one")).await.unwrap();
    assert_eq!(t.seq, highest + 1);
    let other = dry.list(SYS, None, None, None).await.unwrap().into_iter().find(|t| t.epic != biggest).unwrap().epic;
    let highest = store.list(SYS, Some(&other), None, None).await.unwrap().last().unwrap().seq;
    assert_eq!(store.create(SYS, new_ticket(&other, "x")).await.unwrap().seq, highest + 1);
    assert_eq!(store.create(SYS, new_ticket("REAL", "x")).await.unwrap().id, "REAL-2");
    assert_kept_is_a_scan(&store, "the seed and the creates");
}

/// The ticket's second row: creates racing in one shortcode — and in a
/// second one beside it — get distinct ids with no gaps, and what is kept
/// is a scan's when they are done.
#[tokio::test]
async fn concurrent_creates_take_every_id_once_with_no_gaps() {
    let store = Arc::new(Store::new());
    store.create(SYS, scope("RACE", "root")).await.unwrap();
    store.create(SYS, scope("PACE", "root")).await.unwrap();
    let mut handles = Vec::new();
    for i in 0..32 {
        let store = store.clone();
        let epic = if i % 2 == 0 { "RACE" } else { "PACE" };
        handles.push(tokio::spawn(async move {
            store.create(SYS, new_ticket(epic, &format!("t{i}"))).await.unwrap()
        }));
    }
    let mut seqs: HashMap<String, Vec<u32>> = HashMap::new();
    for h in handles {
        let t = h.await.unwrap();
        seqs.entry(t.epic.to_string()).or_default().push(t.seq);
    }
    for (epic, mut got) in seqs {
        got.sort_unstable();
        assert_eq!(got, (2..=17).collect::<Vec<u32>>(), "{epic}: every id once, no gaps");
    }
    assert_kept_is_a_scan(&store, "the race");
}

/// The create path, the long way: 600 pseudo-random creates — forward
/// references to ids a few ahead in the same shortcode and in others,
/// blockers that exist, self-references and cycles through forward
/// references (refused), missing and foreign parents (refused), new
/// shortcodes with and without a scope first — with `set_parent` moves
/// between them. Before every create, a scan says what the newborn must
/// get; after it, the newborn has exactly that id and those `unlocks`, a
/// refusal changed nothing, and the kept indexes are a scan's.
#[tokio::test]
async fn every_create_gets_the_id_and_unlocks_a_scan_would_give_it() {
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) % n as u64) as usize
        }
    }
    const EPICS: [&str; 6] = ["AB", "B", "BA", "SC", "Z", "ZZ"];
    let store = Store::new();
    let mut rng = Rng(0x5343_414C_4533);
    let (mut born, mut refused, mut resolved) = (0, 0, 0);
    for step in 0..600 {
        let (last, unborn) = scanned(&store);
        let all = store.list(SYS, None, None, None).await.unwrap();
        if !all.is_empty() && rng.below(8) == 0 {
            let child = &all[rng.below(all.len())];
            let mine: Vec<&Ticket> = all.iter().filter(|t| t.epic == child.epic).collect();
            let parent = mine[rng.below(mine.len())].id.clone();
            let _ = store.set_parent(SYS.moving(&child.id, &parent)).await;
            assert_kept_is_a_scan(&store, &format!("step {step}: set_parent"));
            continue;
        }
        let epic = EPICS[rng.below(EPICS.len())];
        let seq = last.get(epic).copied().unwrap_or(0) + 1;
        let mut blocked_by = Vec::new();
        for _ in 0..rng.below(3) {
            blocked_by.push(match rng.below(4) {
                // An id up to three ahead, in any of the shortcodes: the
                // next one is this ticket's own (a self-reference).
                0 | 1 => {
                    let e = EPICS[rng.below(EPICS.len())];
                    format!("{e}-{}", last.get(e).copied().unwrap_or(0) + 1 + rng.below(3) as u32)
                }
                _ if !all.is_empty() => all[rng.below(all.len())].id.to_string(),
                _ => format!("{epic}-{seq}"),
            });
        }
        let kind = if seq == 1 || rng.below(6) == 0 {
            [Kind::Scope, Kind::Build][rng.below(2)]
        } else {
            Kind::Build
        };
        // ORG-3: a scope under another shortcode's ticket roots a shortcode
        // there — a namesake, not a ticket of this one; the walk files only
        // a shortcode's first scope so (tests_shortcodes holds the rest).
        let parent = match rng.below(6) {
            0 if !all.is_empty() && (kind != Kind::Scope || seq == 1) => Some(all[rng.below(all.len())].id.to_string()),
            1 => Some(format!("{epic}-99")),
            _ => None,
        };
        let what = format!("step {step}: create {epic}-{seq} {kind} blocked_by {blocked_by:?} under {parent:?}");
        let before = kept(&store);
        let new = NewTicket {
            path: None,
            kind,
            blocked_by: blocked_by.iter().map(|b| b.as_str().into()).collect(),
            parent,
            ..new_ticket(epic, "walked")
        };
        match store.create(SYS, new).await {
            Ok(t) => {
                born += 1;
                assert_eq!(t.seq, seq, "{what}");
                let expected = unborn.get(t.id.as_str()).cloned().unwrap_or_default();
                resolved += usize::from(!expected.is_empty());
                assert_eq!(t.unlocks, expected, "{what}");
            }
            Err(_) => {
                refused += 1;
                assert_eq!(kept(&store), before, "{what}: refused, nothing indexed");
            }
        }
        assert_kept_is_a_scan(&store, &what);
    }
    assert!(born > 250 && refused > 50 && resolved > 30, "{born} born, {refused} refused, {resolved} resolved");
}

/// SCALE-3 reads a shortcode's root from the roll-ups instead of walking
/// the prefix to its first scope. The answer is the same for the shapes
/// only a legacy prefix has: a scope that is not seq 1 is still the
/// default parent, and a prefix that never had a scope still has none.
#[tokio::test]
async fn the_default_parent_is_the_lowest_seq_scope_in_a_legacy_prefix_too() {
    use Kind::{Build, Scope};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-roots.db");
    write_blocked_db(
        &path,
        &[
            ("DEMO-1", Build, &[]),
            ("DEMO-2", Build, &[]),
            ("DEMO-3", Scope, &[]), // the root, not seq 1
            ("DEMO-4", Scope, &[]), // a later scope is not
            ("NOSC-1", Build, &[]),
            ("NOSC-2", Build, &[]),
        ],
    );
    let store = Store::open(&path).unwrap();
    let t = store.create(SYS, new_ticket("DEMO", "defaulted")).await.unwrap();
    assert_eq!((t.id.as_str(), t.parent.as_deref()), ("DEMO-5", Some("DEMO-3")));
    let t = store.create(SYS, new_ticket("NOSC", "no root to default to")).await.unwrap();
    assert_eq!((t.id.as_str(), t.parent.as_deref()), ("NOSC-3", None));
    let t = store.create(SYS, scope("NOSC", "a first scope, late")).await.unwrap();
    assert_eq!(t.parent, None, "a shortcode's root is top-level unless it names a host");
    let t = store.create(SYS, new_ticket("NOSC", "now it has one")).await.unwrap();
    assert_eq!(t.parent.as_deref(), Some("NOSC-4"));
    // MNG-14's guard reads the same root: only it may be hosted elsewhere.
    assert!(store.set_parent(SYS.moving("DEMO-4", "NOSC-4")).await.is_err());
    store.set_parent(SYS.moving("DEMO-3", "NOSC-4")).await.unwrap();
}

// ---- SCALE-16: extents -----------------------------------------------

/// `extents` is the kept sequence as it stands: on a 10⁴ seed, one per
/// shortcode in shortcode order, summing to the count, and row k of every
/// shortcode is `EPIC-k` — so `after: EPIC-k` pages from row k + 1. On a
/// database edited by hand to leave a hole it is still the highest
/// sequence, not a count; and a prefix the nav draws no row for (its
/// tickets all filed under another shortcode's) still has its extent.
#[tokio::test]
async fn extents_are_each_shortcodes_highest_sequence_in_order() {
    let store = Store::new();
    let report = store.seed_synthetic(SYS, spec(10_000, 42)).unwrap();
    let extents = store.extents_text();
    assert_eq!(extents.len(), report.epics);
    assert!(extents.windows(2).all(|w| w[0].0 < w[1].0), "shortcode order");
    assert_eq!(extents.iter().map(|(_, n)| *n as usize).sum::<usize>(), store.count(SYS));
    assert_eq!(extents, kept(&store).0.into_iter().collect::<Vec<_>>());
    for (epic, last) in &extents {
        for k in [1, last / 2, *last] {
            let page = store
                .page(SYS, PageQuery {
                    epic: Some(epic.as_str()),
                    after: Some(&format!("{epic}-{k}")),
                    limit: Some(1),
                    ..PageQuery::default()
                })
                .unwrap();
            let want = (k < *last).then(|| format!("{epic}-{}", k + 1));
            assert_eq!(page.tickets.first().map(|t| t.id.to_string()), want, "{epic} after row {k}");
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("holes.db");
    write_blocked_db(
        &path,
        &[("GAP-1", Kind::Scope, &[]), ("GAP-3", Kind::Build, &[])],
    );
    let store = Store::open(&path).unwrap();
    assert_eq!(store.extents_text(), [("GAP".to_string(), 3)], "the highest, with a hole below it");
    let after = |cursor: &str| {
        store.page(SYS, PageQuery {
            epic: Some("GAP"),
            after: Some(cursor),
            limit: Some(1),
            ..PageQuery::default()
        })
    };
    assert_eq!(ids(&after("GAP-1").unwrap()), ["GAP-3"], "after the row before the hole: the next ticket");
    assert!(matches!(after("GAP-2"), Err(StoreError::CursorNotFound(_))), "the hole itself is no cursor");

    let path = dir.path().join("legacy.db");
    write_db(&path, &legacy());
    let store = Store::open(&path).unwrap();
    assert!(!store.groups(SYS).iter().any(|g| g.epic == "XPC"), "no row for XPC");
    let extents = store.extents_text();
    assert!(extents.contains(&("XPC".to_string(), 2)), "but an extent: {extents:?}");
    assert_eq!(extents.len(), store.list(SYS, None, None, None).await.unwrap().iter().map(|t| t.epic.as_str()).collect::<HashSet<_>>().len());
}
