use super::*;

fn new_ticket(epic: &str, title: &str) -> NewTicket {
    NewTicket {
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
        kind: Kind::Scope,
        ..new_ticket(epic, title)
    }
}

/// `new_ticket`, filed as an output of `parent`.
fn child_of(epic: &str, title: &str, parent: &str) -> NewTicket {
    NewTicket {
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
    store.create(scope("A", "a")).await.unwrap(); // MNG-12: the root first
    for _ in 0..10 {
        store.create(new_ticket("A", "a")).await.unwrap();
    }
    store.create(scope("B", "b")).await.unwrap();
    store.create(scope("AB", "ab")).await.unwrap();

    let all: Vec<String> = store.list(None, None, None).iter().map(|t| t.id.clone()).collect();
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
            .page(PageQuery {
                after: after.as_deref(),
                limit: Some(1),
                ..PageQuery::default()
            })
            .unwrap();
        assert_eq!(page.remaining as usize, all.len() - walked.len() - page.tickets.len());
        walked.extend(page.tickets.iter().map(|t| t.id.clone()));
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
        let id = store.create(nt).await.unwrap().id;
        if i % 2 == 0 {
            store.set_status(&id, Status::Ready).await.unwrap();
        }
    }
    store.create(scope("ZZ", "z")).await.unwrap();

    let page = store
        .page(PageQuery {
            status: Some(Status::Ready),
            after: Some("pg-1"),
            limit: Some(1),
            ..PageQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&page), ["PG-3"], "the cursor is case-insensitive, like every id");
    assert_eq!((page.next.as_deref(), page.remaining), (Some("PG-3"), 1));

    let page = store
        .page(PageQuery {
            epic: Some("PG"),
            after: Some("PG-6"),
            ..PageQuery::default()
        })
        .unwrap();
    assert!(page.tickets.is_empty(), "ZZ-1 is past the epic filter");
    assert_eq!((page.next, page.remaining), (None, 0));

    match store.page(PageQuery {
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
        store.create(scope(epic, "t")).await.unwrap();
        for _ in 0..2 {
            store.create(new_ticket(epic, "t")).await.unwrap();
        }
    }
    let epic_page = |after: Option<&'static str>, limit| {
        store.page(PageQuery {
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
    store.create(scope("MID", "m")).await.unwrap();
    for _ in 0..2 {
        store.create(new_ticket("MID", "m")).await.unwrap();
    }
    store.create(scope("AAA", "below")).await.unwrap();

    let first = store
        .page(PageQuery {
            limit: Some(2),
            ..PageQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&first), ["AAA-1", "MID-1"]);

    // Mid-walk: one above the cursor (MID-4), one below it (AAA-2).
    store.create(new_ticket("MID", "above")).await.unwrap();
    store.create(new_ticket("AAA", "below again")).await.unwrap();

    let rest = store
        .page(PageQuery {
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
        let id = store.create(nt).await.unwrap().id;
        store.set_status(&id, Status::Ready).await.unwrap();
    }
    store
        .create(NewTicket {
            blocked_by: vec!["RD-1".into()],
            ..new_ticket("RD", "blocked")
        })
        .await
        .unwrap();
    store.set_status("RD-5", Status::Ready).await.unwrap();

    let page = store.ready_page(None, None, Some(2)).unwrap();
    assert_eq!(ids(&page), ["RD-1", "RD-2"]);
    assert_eq!((page.next.as_deref(), page.remaining), (Some("RD-2"), 2));

    let page = store.ready_page(None, page.next.as_deref(), Some(2)).unwrap();
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
        assert_eq!(store.count(), 5);
        for (id, kind) in expected {
            let t = store.get(id).unwrap();
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
        assert_eq!(store.get(id).unwrap().kind, kind, "{id} after reopen");
    }
    let next = store.create(new_ticket("LEG", "born after the migration")).await.unwrap();
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
/// parent gains it in `children`, a subscriber sees the parent's
/// change as `ChildrenChanged` carrying the full ticket, and both
/// edges are durable.
#[tokio::test]
async fn a_childs_birth_updates_its_parent_and_is_broadcast() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("family.db");
    let store = Store::open(&path).unwrap();
    let parent = store.create(scope("FAM", "the parent")).await.unwrap();
    assert_eq!(parent.parent, None);
    assert!(parent.children.is_empty());

    let mut rx = store.subscribe();
    let child = store
        .create(child_of("FAM", "the output", "FAM-1"))
        .await
        .unwrap();
    assert_eq!(child.parent.as_deref(), Some("FAM-1"));
    assert!(child.children.is_empty());

    match rx.recv().await.unwrap() {
        Change::Created { ticket } => assert_eq!(ticket.id, "FAM-2"),
        other => panic!("expected Created, got {other:?}"),
    }
    match rx.recv().await.unwrap() {
        Change::ChildrenChanged { ticket, added } => {
            assert_eq!(ticket.id, "FAM-1");
            assert_eq!(added, "FAM-2");
            // The full ticket, not just the delta.
            assert_eq!(ticket.children, vec!["FAM-2"]);
            assert_eq!(ticket.title, "the parent");
            assert_eq!(ticket.status, Status::Pending);
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    assert_eq!(store.get("FAM-1").unwrap().children, vec!["FAM-2"]);

    // Durable: a reopen finds both halves of the edge.
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("FAM-1").unwrap().children, vec!["FAM-2"]);
    assert_eq!(store.get("FAM-2").unwrap().parent.as_deref(), Some("FAM-1"));
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
    store.create(scope("CHAIN", "root")).await.unwrap();
    let err = store
        .create(child_of("CHAIN", "orphan", "CHAIN-9"))
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
    assert_eq!(store.count(), 1, "nothing was created");

    // A ticket cannot name itself: the id is allocated by this very
    // create, so it cannot exist yet — the store refuses it as a
    // missing parent, before any cycle can be formed. The id stays
    // unallocated: the next successful create still takes CHAIN-2.
    let err = store
        .create(child_of("CHAIN", "myself", "CHAIN-2"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::ParentNotFound { .. }),
        "self-parenthood is impossible by construction, got {err:?}"
    );

    // Three deep, the middle link in another epic (its scope root).
    store
        .create(NewTicket {
            kind: Kind::Scope,
            ..child_of("OTHER", "middle", "CHAIN-1")
        })
        .await
        .unwrap();
    // MNG-14: a non-root stays inside its shortcode — the leaf under
    // OTHER-1 is OTHER-2, and a CHAIN ticket naming OTHER-1 is refused.
    let err = store
        .create(child_of("CHAIN", "leaf", "OTHER-1"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::ParentOutsideShortcode { .. }),
        "cross-shortcode parent on a non-root, got {err:?}"
    );
    assert!(err.to_string().contains("OTHER-1") && err.to_string().contains("outside"), "{err}");
    store
        .create(child_of("OTHER", "leaf", "OTHER-1"))
        .await
        .unwrap();
    assert_eq!(store.get("CHAIN-1").unwrap().parent, None);
    assert_eq!(store.get("CHAIN-1").unwrap().children, vec!["OTHER-1"]);
    assert_eq!(store.get("OTHER-1").unwrap().parent.as_deref(), Some("CHAIN-1"));
    assert_eq!(store.get("OTHER-1").unwrap().children, vec!["OTHER-2"]);
    assert_eq!(store.get("OTHER-2").unwrap().parent.as_deref(), Some("OTHER-1"));
    assert!(store.get("OTHER-2").unwrap().children.is_empty());

    // And the guard itself: an ancestry walk that would come back
    // around is caught before anything persists.
    let tickets = Cache::new(
        store
            .list(None, None, None)
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect(),
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
/// both show the fully updated ticket (PERF-4 is exactly this shape).
#[tokio::test]
async fn a_parent_that_is_also_a_blocker_gets_both_edges() {
    let store = Store::new();
    store.create(scope("BOTH", "the parent")).await.unwrap();
    let mut rx = store.subscribe();
    let child = store
        .create(NewTicket {
            blocked_by: vec!["BOTH-1".into()],
            ..child_of("BOTH", "report", "BOTH-1")
        })
        .await
        .unwrap();
    assert_eq!(child.parent.as_deref(), Some("BOTH-1"));
    assert_eq!(child.blocked_by, vec!["BOTH-1"]);

    let parent = store.get("BOTH-1").unwrap();
    assert_eq!(parent.children, vec!["BOTH-2"]);
    assert_eq!(parent.unlocks, vec!["BOTH-2"], "neither edge overwrote the other");

    assert!(matches!(rx.recv().await.unwrap(), Change::Created { .. }));
    match rx.recv().await.unwrap() {
        Change::ChildrenChanged { ticket, .. } => assert_eq!(ticket, parent),
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match rx.recv().await.unwrap() {
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
    store.create(scope("LP", "parent")).await.unwrap();
    store.create(new_ticket("LP", "unrelated")).await.unwrap();
    store.create(child_of("LP", "child a", "LP-1")).await.unwrap();
    store
        .create(NewTicket {
            kind: Kind::Scope,
            ..child_of("ZZ", "child b", "LP-1")
        })
        .await
        .unwrap();
    store.create(child_of("LP", "grandchild", "LP-3")).await.unwrap();
    store.set_status("LP-3", Status::Ready).await.unwrap();

    // MNG-14: "unrelated" (LP-2) named no parent, so it defaulted to
    // the shortcode root and is LP-1's child too.
    let ids = |ts: Vec<Ticket>| ts.into_iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(
        ids(store.list(None, None, Some("LP-1"))),
        vec!["LP-2", "LP-3", "ZZ-1"],
        "exactly LP-1's children, ordered (epic, seq)"
    );
    assert_eq!(ids(store.list(None, None, Some("lp-1"))), vec!["LP-2", "LP-3", "ZZ-1"]);
    assert_eq!(ids(store.list(None, None, Some("LP-3"))), vec!["LP-4"]);
    assert!(store.list(None, None, Some("LP-2")).is_empty(), "no children");
    assert!(store.list(None, None, Some("NOPE-1")).is_empty());
    // Composes with the existing filters.
    assert_eq!(ids(store.list(Some("LP"), None, Some("LP-1"))), vec!["LP-2", "LP-3"]);
    assert_eq!(
        ids(store.list(None, Some(Status::Ready), Some("LP-1"))),
        vec!["LP-3"]
    );
    assert_eq!(store.list(None, None, None).len(), 5, "unfiltered is unchanged");
}

/// Acceptance 3 at the store level: a pre-AGS-19 database gains the
/// columns and its typed parents on first open, from body
/// frontmatter — resolvable ones written through (and mirrored in
/// `children`), an unresolvable one left empty, a frontmatter cycle
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
        assert_eq!(store.count(), 6);
        let p = |id: &str| store.get(id).unwrap().parent;
        assert_eq!(p("OLD-1"), None);
        assert_eq!(p("OLD-2").as_deref(), Some("OLD-1"));
        assert_eq!(p("OLD-3"), None, "an unresolvable parent is left empty");
        assert_eq!(p("OLD-4").as_deref(), Some("OLD-2"));
        // The chain is acyclic: the first half of the OLD-5/OLD-6
        // pair is taken, the half that would close it is refused.
        assert_eq!(p("OLD-5").as_deref(), Some("OLD-6"));
        assert_eq!(p("OLD-6"), None);
        // …and `children` mirrors every accepted edge.
        assert_eq!(store.get("OLD-1").unwrap().children, vec!["OLD-2"]);
        assert_eq!(store.get("OLD-2").unwrap().children, vec!["OLD-4"]);
        assert_eq!(store.get("OLD-6").unwrap().children, vec!["OLD-5"]);
        assert!(store.get("OLD-3").unwrap().children.is_empty());
        // The body is the raw record, untouched by the migration.
        assert!(store.get("OLD-3").unwrap().body.contains("parent: OLD-99"));
    }

    // The file itself carries the typed parents now.
    let cols = columns(&path);
    assert!(cols.contains(&"parent".to_string()), "{cols:?}");
    assert!(cols.contains(&"children".to_string()), "{cols:?}");
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
    assert_eq!(store.get("OLD-4").unwrap().parent.as_deref(), Some("OLD-2"));
    let next = store.create(child_of("OLD", "born after", "OLD-4")).await.unwrap();
    assert_eq!(next.id, "OLD-7");
    assert_eq!(next.parent.as_deref(), Some("OLD-4"));
    assert_eq!(store.get("OLD-4").unwrap().children, vec!["OLD-7"]);
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
fn unknown_kind_error_names_the_five() {
    let err = "bug".parse::<Kind>().unwrap_err();
    for name in ["scope", "execution", "build", "gap", "spike"] {
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
            .create(NewTicket {
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
            .create(child_of("RT", "its output", "RT-1"))
            .await
            .unwrap();
        let parent = match store.set_status("RT-1", Status::Ready).await.unwrap() {
            Change::StatusChanged { ticket, previous } => {
                assert_eq!(previous, Status::Pending);
                ticket
            }
            other => panic!("expected StatusChanged, got {other:?}"),
        };
        assert_eq!(parent.children, vec!["RT-2"]);
        (parent, child)
    };

    // Restart: same location, everything identical.
    let store = Store::open(&path).unwrap();
    assert_eq!(store.count(), 2);
    assert_eq!(store.get("RT-1").unwrap(), after_flip);
    assert_eq!(store.get("RT-2").unwrap(), child);
    assert_eq!(store.get("RT-2").unwrap().parent.as_deref(), Some("RT-1"));

    // Id allocation continues from the persisted max, not from 1.
    let next = store.create(new_ticket("RT", "second")).await.unwrap();
    assert_eq!(next.id, "RT-3");
    assert_eq!(next.seq, 3);
    assert_eq!(next.parent.as_deref(), Some("RT-1"), "MNG-14: defaulted to the root");
    assert!(next.children.is_empty());
}

#[tokio::test]
async fn fresh_location_is_an_empty_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("brand-new.db")).unwrap();
    assert_eq!(store.count(), 0);
    assert!(store.create(scope("NEW", "works")).await.is_ok());
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
    let report = store.seed_synthetic(spec(10_000, 42)).unwrap();
    assert_eq!(report.count, 10_000);
    assert_eq!(store.count(), 10_000);
    assert!(report.epics >= 10, "several epics, got {}", report.epics);
    assert!(report.edges > 0);

    let all = store.list(None, None, None);
    assert_eq!(all.len(), 10_000);
    let by_id: HashMap<String, Ticket> = all.iter().map(|t| (t.id.clone(), t.clone())).collect();

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
            let blocker = by_id.get(b).unwrap_or_else(|| panic!("{}: dangling blocker {b}", t.id));
            assert!(blocker.unlocks.contains(&t.id), "{b}.unlocks lacks {}", t.id);
            edges += 1;
        }
        for u in &t.unlocks {
            let dependent = by_id.get(u).unwrap_or_else(|| panic!("{}: unlocks names {u}", t.id));
            assert!(dependent.blocked_by.contains(&t.id), "{u}.blocked_by lacks {}", t.id);
        }
    }
    assert_eq!(edges, report.edges);
    assert_eq!(has_cycle(&by_id), None);

    // ready_set against an independent computation over `list`.
    let expected: Vec<String> = all
        .iter()
        .filter(|t| t.status == Status::Ready)
        .filter(|t| t.blocked_by.iter().all(|b| satisfied(by_id[b].status)))
        .map(|t| t.id.clone())
        .collect();
    let got: Vec<String> = store.ready_set(None).into_iter().map(|t| t.id).collect();
    assert!(!got.is_empty());
    assert_eq!(got, expected);
    // ...and it is doing real work: some Ready tickets are held back.
    assert!(
        all.iter().filter(|t| t.status == Status::Ready).count() > got.len(),
        "some Ready tickets have unsatisfied blockers"
    );

    // blockers on a sampled ticket with several blockers.
    let sample = all.iter().filter(|t| t.blocked_by.len() >= 2).nth(7).expect("a multi-blocker ticket");
    let refs = store.blockers(&sample.id).unwrap();
    assert_eq!(refs.len(), sample.blocked_by.len());
    for (r, b) in refs.iter().zip(&sample.blocked_by) {
        match r {
            BlockerRef::Resolved(t) => assert_eq!(&t.id, b),
            BlockerRef::Dangling(d) => panic!("{}: {d} dangling", sample.id),
        }
    }

    // Persisted: a reopen finds the same tickets (write-through happened).
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.count(), 10_000);
    assert_eq!(reopened.list(None, None, None), all);
}

/// Criterion 4: the same seed value produces byte-identical tickets;
/// a different one produces different tickets.
#[test]
fn seed_is_deterministic_per_seed_value() {
    let a = Store::new();
    let b = Store::new();
    let c = Store::new();
    a.seed_synthetic(spec(500, 7)).unwrap();
    b.seed_synthetic(spec(500, 7)).unwrap();
    c.seed_synthetic(spec(500, 8)).unwrap();
    let la = a.list(None, None, None);
    assert_eq!(la, b.list(None, None, None));
    assert_ne!(la, c.list(None, None, None));
    assert_eq!(serde_json::to_string(&la).unwrap(), serde_json::to_string(&b.list(None, None, None)).unwrap());
}

/// Criterion 3: a store that already holds tickets refuses the seed
/// (nothing written) unless explicitly allowed — and then ids
/// continue after the existing maxima, never colliding.
#[tokio::test]
async fn seed_refuses_an_existing_store_unless_allowed_then_continues_ids() {
    let store = Store::new();
    store.create(scope("REAL", "already here")).await.unwrap();

    let err = store.seed_synthetic(spec(100, 1)).unwrap_err();
    assert!(matches!(err, StoreError::SeedRefused { existing: 1 }), "{err}");
    assert!(err.to_string().contains("already holds 1 ticket"), "{err}");
    assert_eq!(store.count(), 1, "nothing was written");

    let allowed = SeedSpec {
        into_existing: true,
        ..spec(100, 1)
    };
    store.seed_synthetic(allowed).unwrap();
    assert_eq!(store.count(), 101);
    // Seeding the same spec again regenerates the same epics: the
    // second batch must continue their numbering (the normal
    // allocation), which the UNIQUE (epic, seq) constraint would
    // otherwise reject.
    store.seed_synthetic(allowed).unwrap();
    assert_eq!(store.count(), 201);
    let all = store.list(None, None, None);
    let mut ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 201);
    assert!(store.get("REAL-1").is_some());
    // The normal write path still works afterwards.
    let next = store.create(new_ticket("REAL", "after the seed")).await.unwrap();
    assert_eq!(next.id, "REAL-2");
}

#[test]
fn seed_of_zero_is_a_no_op_and_small_counts_work() {
    let store = Store::new();
    let r = store.seed_synthetic(spec(0, 1)).unwrap();
    assert_eq!((r.count, r.epics, r.edges), (0, 0, 0));
    assert_eq!(store.count(), 0);
    for n in [1, 2, 3, 10] {
        let s = Store::new();
        s.seed_synthetic(spec(n, 1)).unwrap();
        assert_eq!(s.count(), n);
    }
}

#[tokio::test]
async fn concurrent_creates_never_collide_on_ids() {
    let store = Arc::new(Store::new());
    store.create(scope("RACE", "root")).await.unwrap();
    let mut handles = Vec::new();
    for i in 0..16 {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            store
                .create(new_ticket("RACE", &format!("t{i}")))
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
    let err = store.create(new_ticket("FRESH", "a build first")).await.unwrap_err();
    match &err {
        StoreError::FirstMustBeScope { epic, kind } => {
            assert_eq!(epic, "FRESH");
            assert_eq!(*kind, Kind::Build);
        }
        other => panic!("expected FirstMustBeScope, got {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("FRESH") && msg.contains("scope") && msg.contains("build"), "{msg}");
    assert_eq!(store.count(), 0, "nothing was created");

    let root = store.create(scope("fresh", "the root")).await.unwrap();
    assert_eq!(root.id, "FRESH-1", "the refused create allocated nothing");
    assert_eq!(root.kind, Kind::Scope);
    let next = store.create(new_ticket("FRESH", "a build second")).await.unwrap();
    assert_eq!(next.id, "FRESH-2");
    assert_eq!(next.kind, Kind::Build);
    assert_eq!(next.parent.as_deref(), Some("FRESH-1"), "defaults to the shortcode root");
    assert_eq!(store.get("FRESH-1").unwrap().children, vec!["FRESH-2"]);
}

/// MNG-14: a parent named must be inside the ticket's own shortcode;
/// the shortcode's scope root is the exception — its parent is the
/// shortcode it hangs under, in any prefix (a sub-shortcode).
#[tokio::test]
async fn a_parent_stays_inside_the_shortcode_except_for_the_root() {
    let store = Store::new();
    store.create(scope("HOST", "the host")).await.unwrap();
    // A new shortcode's root may hang under HOST-1 …
    let sub = store
        .create(NewTicket {
            kind: Kind::Scope,
            ..child_of("SUB", "a sub-shortcode", "HOST-1")
        })
        .await
        .unwrap();
    assert_eq!(sub.parent.as_deref(), Some("HOST-1"));
    // … but its non-root tickets may not.
    let err = store.create(child_of("SUB", "escapes", "HOST-1")).await.unwrap_err();
    match &err {
        StoreError::ParentOutsideShortcode { id, parent } => {
            assert_eq!(id, "SUB-2");
            assert_eq!(parent, "HOST-1");
        }
        other => panic!("expected ParentOutsideShortcode, got {other:?}"),
    }
    assert_eq!(store.count(), 2, "nothing was created");
    // Nor be moved out afterwards; the root can be re-hosted.
    store.create(new_ticket("SUB", "inside")).await.unwrap(); // SUB-2 → SUB-1
    assert!(matches!(
        store.set_parent("SUB-2", "HOST-1").await.unwrap_err(),
        StoreError::ParentOutsideShortcode { .. }
    ));
    store.create(scope("HOST2", "another host")).await.unwrap();
    store.set_parent("SUB-1", "HOST2-1").await.unwrap();
    assert_eq!(store.get("SUB-1").unwrap().parent.as_deref(), Some("HOST2-1"));
    assert_eq!(store.get("HOST-1").unwrap().children, Vec::<String>::new());
    assert_eq!(store.get("HOST2-1").unwrap().children, vec!["SUB-1"]);
}

/// MNG-12 criterion 2: `set_parent` moves a ticket — the child, the
/// new parent and the old parent move together, the three events carry
/// full tickets, the edge survives a reopen, and repeating it is a
/// no-op. (Adopting a *parentless* non-root is the MNG-12 backfill's
/// case; since MNG-14 none is born that way.)
#[tokio::test]
async fn set_parent_moves_a_ticket_broadcasts_and_is_durable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("move.db");
    let store = Store::open(&path).unwrap();
    store.create(scope("AD", "the root")).await.unwrap(); // AD-1
    store.create(new_ticket("AD", "a")).await.unwrap(); // AD-2 → AD-1
    store.create(new_ticket("AD", "b")).await.unwrap(); // AD-3 → AD-1
    assert_eq!(store.get("AD-1").unwrap().children, vec!["AD-2", "AD-3"]);
    let mut rx = store.subscribe();

    match store.set_parent("ad-3", "ad-2").await.unwrap() {
        Change::ParentChanged { ticket, previous } => {
            assert_eq!(ticket.id, "AD-3");
            assert_eq!(ticket.parent.as_deref(), Some("AD-2"));
            assert_eq!(previous.as_deref(), Some("AD-1"));
        }
        other => panic!("expected ParentChanged, got {other:?}"),
    }
    match rx.recv().await.unwrap() {
        Change::ParentChanged { ticket, .. } => {
            assert_eq!(ticket.parent.as_deref(), Some("AD-2"))
        }
        other => panic!("expected ParentChanged, got {other:?}"),
    }
    match rx.recv().await.unwrap() {
        Change::ChildrenChanged { ticket, added } => {
            assert_eq!(ticket.id, "AD-2");
            assert_eq!(added, "AD-3");
            assert_eq!(ticket.children, vec!["AD-3"]);
            assert_eq!(ticket.title, "a", "the full parent, not a delta");
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match rx.recv().await.unwrap() {
        Change::ChildRemoved { ticket, removed } => {
            assert_eq!(ticket.id, "AD-1");
            assert_eq!(removed, "AD-3");
            assert_eq!(ticket.children, vec!["AD-2"], "the full ex-parent");
        }
        other => panic!("expected ChildRemoved, got {other:?}"),
    }
    assert!(rx.try_recv().is_err());

    // Idempotent: the parent it already has writes and emits nothing.
    match store.set_parent("AD-3", "AD-2").await.unwrap() {
        Change::ParentChanged { previous, .. } => assert_eq!(previous.as_deref(), Some("AD-2")),
        other => panic!("expected ParentChanged, got {other:?}"),
    }
    assert!(rx.try_recv().is_err(), "a no-op emits nothing");

    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("AD-3").unwrap().parent.as_deref(), Some("AD-2"));
    assert_eq!(store.get("AD-2").unwrap().children, vec!["AD-3"]);
    assert_eq!(store.get("AD-1").unwrap().children, vec!["AD-2"]);
}

/// MNG-12 criterion 3: the guards, and a move. A missing child, a
/// parent that names no ticket, and any chain that comes back around
/// (itself included) are refused with nothing changed; moving a child
/// removes it from the old parent and says so.
#[tokio::test]
async fn set_parent_guards_ancestry_and_a_move_removes_from_the_old_parent() {
    let store = Store::new();
    store.create(scope("MV", "root")).await.unwrap(); // MV-1
    store.create(child_of("MV", "a", "MV-1")).await.unwrap(); // MV-2
    store.create(child_of("MV", "b", "MV-2")).await.unwrap(); // MV-3
    store.create(new_ticket("MV", "loose")).await.unwrap(); // MV-4 → MV-1 (MNG-14)

    assert!(matches!(
        store.set_parent("MV-9", "MV-1").await.unwrap_err(),
        StoreError::NotFound(_)
    ));
    match store.set_parent("MV-4", "MV-9").await.unwrap_err() {
        StoreError::ParentNotFound { id, parent } => {
            assert_eq!(id, "MV-4");
            assert_eq!(parent, "MV-9");
        }
        other => panic!("expected ParentNotFound, got {other:?}"),
    }
    match store.set_parent("MV-2", "MV-2").await.unwrap_err() {
        StoreError::ParentCycle { id, path } => {
            assert_eq!(id, "MV-2");
            assert_eq!(path, vec!["MV-2"]);
        }
        other => panic!("expected ParentCycle, got {other:?}"),
    }
    match store.set_parent("MV-1", "MV-3").await.unwrap_err() {
        StoreError::ParentCycle { id, path } => {
            assert_eq!(id, "MV-1");
            assert_eq!(path, vec!["MV-3", "MV-2", "MV-1"]);
            let msg = StoreError::ParentCycle { id, path }.to_string();
            assert!(msg.contains("own ancestor") && msg.contains("MV-3 -> MV-2 -> MV-1"), "{msg}");
        }
        other => panic!("expected ParentCycle, got {other:?}"),
    }
    assert_eq!(store.get("MV-1").unwrap().parent, None, "nothing changed");
    assert_eq!(store.get("MV-2").unwrap().children, vec!["MV-3"]);

    // The move: MV-3 from under MV-2 to under MV-1.
    let mut rx = store.subscribe();
    store.set_parent("MV-3", "MV-1").await.unwrap();
    assert!(matches!(
        rx.recv().await.unwrap(),
        Change::ParentChanged { previous: Some(p), .. } if p == "MV-2"
    ));
    match rx.recv().await.unwrap() {
        Change::ChildrenChanged { ticket, added } => {
            assert_eq!((ticket.id.as_str(), added.as_str()), ("MV-1", "MV-3"));
            assert_eq!(ticket.children, vec!["MV-2", "MV-4", "MV-3"]);
        }
        other => panic!("expected ChildrenChanged, got {other:?}"),
    }
    match rx.recv().await.unwrap() {
        Change::ChildRemoved { ticket, removed } => {
            assert_eq!((ticket.id.as_str(), removed.as_str()), ("MV-2", "MV-3"));
            assert!(ticket.children.is_empty(), "the full ex-parent");
        }
        other => panic!("expected ChildRemoved, got {other:?}"),
    }
    assert_eq!(store.get("MV-3").unwrap().parent.as_deref(), Some("MV-1"));
    assert_eq!(store.get("MV-2").unwrap().children, Vec::<String>::new());
}

// ---- MNG-10: the roll-ups are the nav's plan() -----------------------

use crate::nav_plan::{self, Facts};
use std::collections::BTreeSet;

/// What the nav draws for this store's rows: AGS-20's `plan()` over
/// every ticket, as groups.
fn nav(store: &Store) -> Vec<Group> {
    let all = store.list(None, None, None);
    let facts: Vec<Facts> = all
        .iter()
        .map(|t| Facts {
            id: t.id.clone(),
            epic: t.epic.clone(),
            seq: t.seq,
            kind: t.kind.to_string(),
            parent: t.parent.clone(),
        })
        .collect();
    let complete: BTreeSet<String> = all
        .iter()
        .filter(|t| t.status == Status::Complete)
        .map(|t| t.id.clone())
        .collect();
    nav_plan::rows(&facts, &complete)
        .into_iter()
        .map(|r| Group {
            epic: r.epic,
            total: r.total,
            done: r.done,
            root: r.root,
            host: r.host,
        })
        .collect()
}

/// Every `GroupChanged` waiting on `rx`, in the order sent (the ticket
/// changes around them drained and dropped).
fn group_changes(rx: &mut broadcast::Receiver<Change>) -> Vec<Group> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_recv() {
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
                host: None,
                ..g.clone()
            });
        }
    }
    out.sort_by(|a, b| a.epic.cmp(&b.epic));
    out
}

fn group(epic: &str, total: u64, done: u64, root: &str, host: Option<&str>) -> Group {
    Group {
        epic: epic.into(),
        total,
        done,
        root: Some(root.into()),
        host: host.map(Into::into),
    }
}

/// The oracle is the nav's function: the fixtures `nav.rs` pins its
/// forest with draw the same subtrees here.
#[test]
fn the_oracle_draws_the_navs_own_fixtures() {
    let f = |id: &str, kind: &str, parent: Option<&str>| {
        let (epic, seq) = id.split_once('-').unwrap();
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
        kind: Kind::Scope,
        ..child_of(epic, title, host)
    };
    store.create(scope("PLEX", "plexus")).await.unwrap(); // PLEX-1
    store.create(new_ticket("PLEX", "core")).await.unwrap(); // PLEX-2 → PLEX-1
    store.create(sub("TRAN", "transport", "PLEX-1")).await.unwrap(); // TRAN-1
    store.create(new_ticket("TRAN", "ws")).await.unwrap(); // TRAN-2 → TRAN-1
    store.create(new_ticket("TRAN", "rest")).await.unwrap(); // TRAN-3 → TRAN-1
    store.create(sub("WIRE", "wire", "TRAN-2")).await.unwrap(); // WIRE-1
    store.create(new_ticket("WIRE", "frames")).await.unwrap(); // WIRE-2 → WIRE-1
    store.create(scope("SOLO", "alone")).await.unwrap(); // SOLO-1
    store.create(new_ticket("SOLO", "one")).await.unwrap(); // SOLO-2 → SOLO-1

    assert_eq!(
        store.groups(),
        vec![
            group("PLEX", 7, 0, "PLEX-1", None),
            group("SOLO", 2, 0, "SOLO-1", None),
            group("TRAN", 5, 0, "TRAN-1", Some("PLEX-1")),
            group("WIRE", 2, 0, "WIRE-1", Some("TRAN-2")),
        ]
    );
    assert_eq!(store.groups(), nav(&store));
    let mut rx = store.subscribe();

    // Acceptance 2: one shortcode, one event, done + 1, after the
    // status change it follows from.
    store.set_status("SOLO-2", Status::Complete).await.unwrap();
    assert!(matches!(rx.try_recv().unwrap(), Change::StatusChanged { .. }));
    match rx.try_recv().unwrap() {
        Change::GroupChanged { group: g } => assert_eq!(g, group("SOLO", 2, 1, "SOLO-1", None)),
        other => panic!("expected GroupChanged, got {other:?}"),
    }
    assert!(rx.try_recv().is_err(), "one group moved, one event");

    // Not a crossing of Complete: nothing moves.
    store.set_status("WIRE-2", Status::Ready).await.unwrap();
    assert!(matches!(rx.try_recv().unwrap(), Change::StatusChanged { .. }));
    assert!(rx.try_recv().is_err(), "Pending → Ready moves no count");

    // Deepest first: WIRE, the TRAN row it nests in, then PLEX.
    store.set_status("WIRE-2", Status::Complete).await.unwrap();
    assert_eq!(
        group_changes(&mut rx),
        vec![
            group("WIRE", 2, 1, "WIRE-1", Some("TRAN-2")),
            group("TRAN", 5, 1, "TRAN-1", Some("PLEX-1")),
            group("PLEX", 7, 1, "PLEX-1", None),
        ]
    );
    // Superseded is not done (the nav's rule): back out the same way.
    store.set_status("WIRE-2", Status::Superseded).await.unwrap();
    let done: Vec<(String, u64)> =
        group_changes(&mut rx).into_iter().map(|g| (g.epic, g.done)).collect();
    assert_eq!(done, [("WIRE".into(), 0), ("TRAN".into(), 0), ("PLEX".into(), 0)]);

    // A birth deep inside counts in every row around it.
    store.create(new_ticket("WIRE", "more")).await.unwrap(); // WIRE-3
    let totals: Vec<(String, u64)> =
        group_changes(&mut rx).into_iter().map(|g| (g.epic, g.total)).collect();
    assert_eq!(totals, [("WIRE".into(), 3), ("TRAN".into(), 6), ("PLEX".into(), 8)]);
    assert_eq!(store.groups(), nav(&store));

    // Re-hosting a sub-shortcode moves its whole count: WIRE from under
    // TRAN-2 to under SOLO-1. After the ticket events, every row that
    // moved — innermost first, then shortcode order.
    store.set_parent("WIRE-1", "SOLO-1").await.unwrap();
    assert!(matches!(rx.try_recv().unwrap(), Change::ParentChanged { .. }));
    assert!(matches!(rx.try_recv().unwrap(), Change::ChildrenChanged { .. }));
    assert!(matches!(rx.try_recv().unwrap(), Change::ChildRemoved { .. }));
    assert_eq!(
        group_changes(&mut rx),
        vec![
            group("TRAN", 3, 0, "TRAN-1", Some("PLEX-1")),
            group("WIRE", 3, 0, "WIRE-1", Some("SOLO-1")),
            group("PLEX", 5, 0, "PLEX-1", None),
            group("SOLO", 5, 1, "SOLO-1", None),
        ]
    );
    assert_eq!(store.groups(), nav(&store));

    // A top-level shortcode becomes a sub-shortcode: SOLO (and WIRE
    // inside it) under PLEX-2. Its pool moves whole.
    store.set_parent("SOLO-1", "PLEX-2").await.unwrap();
    assert_eq!(
        store.groups(),
        vec![
            group("PLEX", 10, 1, "PLEX-1", None),
            group("SOLO", 5, 1, "SOLO-1", Some("PLEX-2")),
            group("TRAN", 3, 0, "TRAN-1", Some("PLEX-1")),
            group("WIRE", 3, 0, "WIRE-1", Some("SOLO-1")),
        ]
    );
    assert_eq!(store.groups(), nav(&store));
    let epics: Vec<String> = group_changes(&mut rx).into_iter().map(|g| g.epic).collect();
    assert_eq!(epics, ["SOLO", "PLEX"], "WIRE's own row did not move");
}

/// A database the server could not write today but must still load,
/// written straight through the schema — `children` mirrored from
/// `parent`, as the store keeps it.
fn write_db(path: &Path, rows: &[(&str, Kind, Status, Option<&str>)]) {
    let mut tickets: Vec<Ticket> = rows
        .iter()
        .map(|(id, kind, status, parent)| {
            let (epic, seq) = id.rsplit_once('-').unwrap();
            Ticket {
                id: id.to_string(),
                epic: epic.into(),
                seq: seq.parse().unwrap(),
                title: id.to_string(),
                status: *status,
                kind: *kind,
                severity: None,
                blocked_by: Vec::new(),
                unlocks: Vec::new(),
                parent: parent.map(Into::into),
                children: Vec::new(),
                body: String::new(),
                created_unix: 0,
                updated_unix: 0,
            }
        })
        .collect();
    let edges: Vec<(String, Option<String>)> =
        tickets.iter().map(|t| (t.id.clone(), t.parent.clone())).collect();
    for t in &mut tickets {
        t.children = edges
            .iter()
            .filter(|(_, p)| p.as_deref() == Some(t.id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
    }
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
fn legacy() -> Vec<(&'static str, Kind, Status, Option<&'static str>)> {
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
    let drawn = store.groups();
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
    store.set_parent("CYC-2", "CYC-4").await.unwrap();
    assert_eq!(store.groups(), nav(&store));
    assert_eq!(group_changes(&mut rx), Vec::<Group>::new());

    // Off the cross cycle: CXA's root under PLEX-2 frees the other two
    // — CXC-1's parent CXA-1 makes CXC a sub-shortcode of CXA, and
    // CXB-1's parent CXC-1 one of CXC: three rows nest into PLEX at
    // once, each inside the last.
    let before = nav(&store);
    store.set_parent("CXA-1", "PLEX-2").await.unwrap();
    let after = nav(&store);
    assert_eq!(store.groups(), after);
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
    got.sort_by(|a, b| a.epic.cmp(&b.epic));
    assert_eq!(got, moved(&before, &after));

    // The ghost is born: ORPH-2 now hangs under it, in GHOST's row.
    let before = nav(&store);
    store.create(scope("GHOST", "late")).await.unwrap();
    let after = nav(&store);
    assert_eq!(store.groups(), after);
    assert_eq!(
        after.iter().find(|g| g.epic == "GHOST").map(|g| (g.total, g.done)),
        Some((2, 1))
    );
    let mut got = group_changes(&mut rx);
    got.sort_by(|a, b| a.epic.cmp(&b.epic));
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
    assert_eq!(store.groups(), nav(&store));
    let mut rx = store.subscribe();
    let mut rng = Rng(0x4D4E_4731);
    let (mut writes, mut moves, mut rehosts) = (0, 0, 0);

    for step in 0..800 {
        let all = store.list(None, None, None);
        let before = nav(&store);
        let any = |rng: &mut Rng| all[rng.below(all.len())].id.clone();
        let what = match rng.below(10) {
            0..=3 => {
                let epic = EPICS[rng.below(EPICS.len())];
                let kind = if rng.below(3) == 0 { Kind::Scope } else { Kind::Build };
                let mine: Vec<&Ticket> = all.iter().filter(|t| t.epic == epic).collect();
                let parent = match rng.below(5) {
                    0 | 1 => None,
                    2 | 3 if !mine.is_empty() => Some(mine[rng.below(mine.len())].id.clone()),
                    _ => Some(any(&mut rng)),
                };
                let what = format!("create {epic} {kind} under {parent:?}");
                let new = NewTicket {
                    kind,
                    parent,
                    ..new_ticket(epic, "walked")
                };
                if store.create(new).await.is_ok() {
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
                store.set_status(&id, status).await.unwrap();
                writes += 1;
                format!("set_status {id} {status}")
            }
            _ => {
                // A root a third of the time: re-hosting shortcodes is
                // the reshaping the roll-ups must follow.
                let child = if rng.below(3) == 0 {
                    before[rng.below(before.len())].root.clone().unwrap_or_else(|| any(&mut rng))
                } else {
                    any(&mut rng)
                };
                let epic = store.get(&child).unwrap().epic;
                let mine: Vec<&Ticket> = all.iter().filter(|t| t.epic == epic).collect();
                let parent = if rng.below(2) == 0 {
                    mine[rng.below(mine.len())].id.clone()
                } else {
                    any(&mut rng)
                };
                let was = store.get(&child).unwrap().parent;
                if store.set_parent(&child, &parent).await.is_ok()
                    && was.as_deref() != Some(parent.as_str())
                {
                    writes += 1;
                    moves += 1;
                    if store.get(&parent).unwrap().epic != epic {
                        rehosts += 1;
                    }
                }
                format!("set_parent {child} under {parent}")
            }
        };
        let after = nav(&store);
        assert_eq!(store.groups(), after, "step {step}: {what}");
        let mut got = group_changes(&mut rx);
        got.sort_by(|a, b| a.epic.cmp(&b.epic));
        assert_eq!(got, moved(&before, &after), "step {step}: {what}");
    }
    assert!(writes > 400 && moves > 40 && rehosts > 5, "{writes} writes, {moves} moves, {rehosts} re-hosts");

    let kept = store.groups();
    drop(store);
    assert_eq!(Store::open(&path).unwrap().groups(), kept, "a reopen rebuilds the same");
}

/// The build at scale: a 10⁴ seed rolls up as the nav draws it — every
/// ticket in exactly one top-level row — and stays so when one of its
/// shortcodes is hosted under another and when a second seed lands in
/// the same store.
#[tokio::test]
async fn a_seeded_store_rolls_up_as_the_nav_draws_it() {
    let store = Store::new();
    let report = store.seed_synthetic(spec(10_000, 42)).unwrap();
    let groups = store.groups();
    assert_eq!(groups.len(), report.epics);
    assert_eq!(groups, nav(&store));
    let top: u64 = groups.iter().filter(|g| g.host.is_none()).map(|g| g.total).sum();
    assert_eq!(top, 10_000, "every ticket drawn in exactly one top-level row");

    let host = format!("{}-2", groups[0].epic);
    store
        .set_parent(groups[1].root.as_deref().unwrap(), &host)
        .await
        .unwrap();
    assert_eq!(store.groups(), nav(&store));
    assert_eq!(store.groups()[1].host.as_deref(), Some(host.as_str()));

    store
        .seed_synthetic(SeedSpec {
            into_existing: true,
            ..spec(500, 3)
        })
        .unwrap();
    assert_eq!(store.groups(), nav(&store));
}
