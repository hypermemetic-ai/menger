//! SDLC-9: the commits that finished a ticket, on its row. `link_commits`
//! and `unlink_commits` go through the write-through path every other
//! write takes — durable, then the cache, then one event — and these hold
//! them to the ticket's table: the reference grammar, all or nothing, the
//! no-ops that write and say nothing, the bound, link order on every path
//! that answers a row, nothing else about the ticket moving, and a
//! database from before the column.

use super::*;

fn ticket(epic: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: Some(Severity::Low),
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(Into::into),
        body: "## Problem\n\nünïcode ✓ 𝔘\r\n\tand no final newline".into(),
    }
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn linked(change: Change) -> (Ticket, Vec<String>) {
    match change {
        Change::CommitsLinked { ticket, added } => (ticket, added),
        other => panic!("expected CommitsLinked, got {other:?}"),
    }
}

fn unlinked(change: Change) -> (Ticket, Vec<String>) {
    match change {
        Change::CommitsUnlinked { ticket, removed } => (ticket, removed),
        other => panic!("expected CommitsUnlinked, got {other:?}"),
    }
}

/// The grammar, both ways: what is a reference, and the text it is kept and
/// compared as (the hash lowercased, the repository as given); and what is
/// not — nothing is trimmed, one `@`, a non-empty repository of letters,
/// digits, `.`, `_`, `-`, and 7 to 64 hex digits after it.
#[test]
fn a_reference_is_seven_to_sixty_four_hex_digits_after_an_optional_repository() {
    let sha256 = "a".repeat(64);
    for (entry, kept) in [
        ("77df9a0", "77df9a0"),
        ("77DF9A0", "77df9a0"),
        ("4f7114a3c9d2e8b1f0a6c5d4e3b2a1f0e9d8c7b6", "4f7114a3c9d2e8b1f0a6c5d4e3b2a1f0e9d8c7b6"),
        (sha256.as_str(), sha256.as_str()),
        ("plexus-macros@4F7114A", "plexus-macros@4f7114a"),
        ("Plexus_Core.rs@0bb28b2", "Plexus_Core.rs@0bb28b2"),
        ("a@1234567", "a@1234567"),
    ] {
        assert_eq!(commits::reference(entry).as_deref(), Some(kept), "{entry}");
    }
    let too_long = "a".repeat(65);
    for not in [
        "",
        "77df9a", // six: too short
        too_long.as_str(),
        "77df9g0", // not hex
        " 77df9a0", // nothing is trimmed
        "77df9a0\n",
        "@77df9a0", // an empty repository
        "plexus/macros@77df9a0", // `/` is not in a repository name
        "plexus macros@77df9a0",
        "a@b@77df9a0", // one `@`
        "77df9a0@plexus", // the hash comes last
        "HEAD",
        "v1.2.3",
        "plexus-macros@",
        "ünï@77df9a0",
    ] {
        assert_eq!(commits::reference(not), None, "{not:?} is not a reference");
    }
}

/// The table, row by row. Each link or unlink that changes something is
/// exactly one event on the channel, in acknowledgment order, and no
/// `group_changed`; a no-op, a refusal and a miss put nothing on it and
/// write nothing.
#[tokio::test]
async fn every_row_of_the_table() {
    let store = Store::new();
    store.create(ticket("CM", Kind::Scope, None, &[])).await.unwrap(); // CM-1
    store.create(ticket("CM", Kind::Build, None, &[])).await.unwrap(); // CM-2
    let mut rx = store.subscribe();
    let next = |rx: &mut broadcast::Receiver<Change>| {
        let change = rx.try_recv().expect("one event");
        assert!(rx.try_recv().is_err(), "exactly one, and no group moved");
        change
    };

    // Two, appended in the order given.
    let (t, added) = linked(store.link_commits("cm-2", strings(&["77df9a0", "Plexus-Macros@4F7114A"])).await.unwrap());
    assert_eq!(added, ["77df9a0", "Plexus-Macros@4f7114a"], "the hash lowercased, the repository as given");
    assert_eq!(t.commits, added);
    assert_eq!(linked(next(&mut rx)), (t, added));

    // One of them again with a third: only the third is added, at the end.
    let (t, added) = linked(store.link_commits("CM-2", strings(&["77DF9A0", "0bb28b2"])).await.unwrap());
    assert_eq!(added, ["0bb28b2"]);
    assert_eq!(t.commits, ["77df9a0", "Plexus-Macros@4f7114a", "0bb28b2"], "link order");
    assert_eq!(linked(next(&mut rx)), (t.clone(), added));
    assert_eq!(store.get("CM-2").await.unwrap(), t, "get shows them");

    // Every entry already linked; none given; one named twice in a call.
    for entries in [vec!["77df9a0"], vec!["0BB28B2", "Plexus-Macros@4f7114a"], vec![]] {
        let (got, added) = linked(store.link_commits("CM-2", strings(&entries)).await.unwrap());
        assert!(added.is_empty(), "{entries:?}: {added:?}");
        assert_eq!(got, t, "{entries:?}: the ticket as it is, unwritten");
        assert!(rx.try_recv().is_err(), "{entries:?}: a no-op broadcasts nothing");
    }
    let (t, added) = linked(store.link_commits("CM-2", strings(&["5054a51", "5054A51"])).await.unwrap());
    assert_eq!(added, ["5054a51"], "an entry named twice is linked once");
    assert_eq!(t.commits.len(), 4);
    linked(next(&mut rx));

    // One malformed entry among valid ones: refused whole, naming it.
    let before = store.get("CM-2").await.unwrap();
    for entries in [vec!["499e3a0", "not-a-commit", "c8c5678"], vec!["499e3a0", "abc"]] {
        match store.link_commits("CM-2", strings(&entries)).await {
            Err(e @ StoreError::NotACommit { .. }) => {
                let bad = entries[1];
                assert!(e.to_string().contains(&format!("'{bad}'")) && e.to_string().contains("CM-2"), "{e}");
                assert!(!e.to_string().contains("'499e3a0'"), "names only what is wrong: {e}");
            }
            other => panic!("{entries:?}: expected NotACommit, got {other:?}"),
        }
    }
    match store.link_commits("CM-2", strings(&["xyz", "499e3a0", "HEAD"])).await {
        Err(StoreError::NotACommit { entries, .. }) => assert_eq!(entries, ["xyz", "HEAD"], "every one, as given"),
        other => panic!("expected NotACommit, got {other:?}"),
    }
    assert_eq!(store.get("CM-2").await.unwrap(), before, "the refused links wrote nothing");
    assert!(rx.try_recv().is_err());

    // Unlink: what is on the ticket comes off, in the order given; a
    // reference that is not linked is not an error, and is not removed.
    let (t, removed) = unlinked(store.unlink_commits("CM-2", strings(&["0BB28B2", "c8c5678", "77df9a0"])).await.unwrap());
    assert_eq!(removed, ["0bb28b2", "77df9a0"]);
    assert_eq!(t.commits, ["Plexus-Macros@4f7114a", "5054a51"], "the rest, still in link order");
    assert_eq!(unlinked(next(&mut rx)), (t.clone(), removed));
    for entries in [vec!["c8c5678"], vec!["plexus-macros@4f7114a"], vec![]] {
        let (got, removed) = unlinked(store.unlink_commits("CM-2", strings(&entries)).await.unwrap());
        assert!(removed.is_empty(), "{entries:?}: compared as stored text — {removed:?}");
        assert_eq!(got, t);
        assert!(rx.try_recv().is_err(), "{entries:?}: nothing removed, nothing said");
    }
    assert!(matches!(
        store.unlink_commits("CM-2", strings(&["5054a51", "nope"])).await,
        Err(StoreError::NotACommit { entries, .. }) if entries == ["nope"]
    ));
    assert_eq!(store.get("CM-2").await.unwrap(), t, "the refused unlink removed nothing");

    // An id naming no ticket, with entries or without.
    for entries in [vec!["77df9a0"], vec![]] {
        assert!(matches!(store.link_commits("CM-9", strings(&entries)).await, Err(StoreError::NotFound(id)) if id == "CM-9"));
        assert!(matches!(store.unlink_commits("CM-9", strings(&entries)).await, Err(StoreError::NotFound(id)) if id == "CM-9"));
    }
    assert!(rx.try_recv().is_err());
}

/// "Linking moves `updated_unix` and nothing else": on a ticket with every
/// edge and a worker status, a link and an unlink move its commits and its
/// timestamp and nothing else on it or on any other ticket — no status, no
/// roll-up, no sequence, no forward reference. And a `Complete` ticket
/// takes none at all without complaint: no status requires one.
#[tokio::test]
async fn a_link_moves_the_commits_and_updated_unix_and_nothing_else() {
    let store = Store::new();
    store.create(ticket("ED", Kind::Scope, None, &[])).await.unwrap(); // ED-1
    store.create(ticket("ED", Kind::Build, None, &[])).await.unwrap(); // ED-2
    store.create(ticket("ED", Kind::Gap, Some("ED-1"), &["ED-2", "ED-9"])).await.unwrap(); // ED-3
    store.create(ticket("ED", Kind::Spike, Some("ED-3"), &["ED-3"])).await.unwrap(); // ED-4
    store.set_status("ED-3", Status::Coding).await.unwrap();
    store.set_status("ED-2", Status::Complete).await.unwrap();
    let before: Vec<Ticket> = store.list(None, None, None).await.unwrap();
    let (groups, extents) = (store.groups(), store.extents_text());
    let started = chrono::Utc::now().timestamp();

    let (t, _) = linked(store.link_commits("ED-3", strings(&["77df9a0", "ought@1a2b3c4"])).await.unwrap());
    let was = before.iter().find(|b| b.id == "ED-3").unwrap();
    assert!(t.updated_unix >= started);
    let mut expect = was.clone();
    expect.commits = strings(&["77df9a0", "ought@1a2b3c4"]);
    expect.updated_unix = t.updated_unix;
    assert_eq!(t, expect, "status (Coding), kind, body, edges, parent, children: as they were");
    let (t, _) = unlinked(store.unlink_commits("ED-3", strings(&["77df9a0"])).await.unwrap());
    expect.commits = strings(&["ought@1a2b3c4"]);
    expect.updated_unix = t.updated_unix;
    assert_eq!(t, expect);

    let after = store.list(None, None, None).await.unwrap();
    for (b, a) in before.iter().zip(&after) {
        if a.id != "ED-3" {
            assert_eq!(a, b, "{} untouched", a.id);
        }
    }
    assert_eq!(store.get("ED-2").await.unwrap().commits, Vec::<String>::new(), "Complete with no commit");
    assert_eq!(store.groups(), groups, "no roll-up moved");
    assert_eq!(store.extents_text(), extents, "no sequence moved");
    for _ in 5..=9 {
        store.create(ticket("ED", Kind::Build, None, &[])).await.unwrap();
    }
    assert_eq!(store.get("ED-9").await.unwrap().unlocks, ["ED-3"], "the forward reference kept");
}

/// A ticket holds at most 256: a link that would pass it is refused whole
/// — not the part that fits — and one that reaches it exactly is taken;
/// at the bound, a link of what is already there is still a no-op, and an
/// unlink makes room.
#[tokio::test]
async fn a_ticket_holds_at_most_256_commits() {
    let store = Store::new();
    store.create(ticket("BD", Kind::Scope, None, &[])).await.unwrap();
    let hash = |i: usize| format!("{i:07x}");
    let first: Vec<String> = (0..255).map(hash).collect();
    let (t, added) = linked(store.link_commits("BD-1", first.clone()).await.unwrap());
    assert_eq!((added.len(), t.commits.len()), (255, 255));
    let mut rx = store.subscribe();

    match store.link_commits("BD-1", vec![hash(0), hash(255), hash(256)]).await {
        Err(e @ StoreError::TooManyCommits { linked: 255, adding: 2, .. }) => {
            assert!(e.to_string().contains("256") && e.to_string().contains("BD-1"), "{e}")
        }
        other => panic!("expected TooManyCommits, got {other:?}"),
    }
    assert_eq!(store.get("BD-1").await.unwrap().commits, first, "nothing written, not even the one that fit");
    assert!(rx.try_recv().is_err());

    let (t, added) = linked(store.link_commits("BD-1", vec![hash(0), hash(255)]).await.unwrap());
    assert_eq!((added, t.commits.len()), (vec![hash(255)], commits::MAX_PER_TICKET));
    let (_, added) = linked(store.link_commits("BD-1", vec![hash(7), hash(255)]).await.unwrap());
    assert!(added.is_empty(), "at the bound, what is there is still a no-op");
    assert!(matches!(
        store.link_commits("BD-1", vec![hash(256)]).await,
        Err(StoreError::TooManyCommits { linked: 256, adding: 1, .. })
    ));
    unlinked(store.unlink_commits("BD-1", vec![hash(3)]).await.unwrap());
    let (t, _) = linked(store.link_commits("BD-1", vec![hash(256)]).await.unwrap());
    assert_eq!(t.commits.len(), 256);
    assert_eq!(t.commits.last(), Some(&hash(256)));
}

/// Link order on every path that answers a ticket or a row — `get`, a walk,
/// a summary page, `find`, the event — through the file and a reopen; the
/// column holds the canonical text as a JSON list, and every ticket never
/// linked reads `[]`.
#[tokio::test]
async fn commits_are_on_every_row_in_link_order_and_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commits.db");
    let linked_at = {
        let store = Store::open(&path).unwrap();
        store.create(ticket("RO", Kind::Scope, None, &[])).await.unwrap();
        store.create(ticket("RO", Kind::Build, None, &[])).await.unwrap();
        store.link_commits("RO-2", strings(&["C8C5678", "plexus-core@0BB28B2"])).await.unwrap();
        let (t, _) = linked(store.link_commits("RO-2", strings(&["090cdec"])).await.unwrap());
        assert_eq!(t.commits, ["c8c5678", "plexus-core@0bb28b2", "090cdec"]);
        t
    };
    let stored: String = Connection::open(&path)
        .unwrap()
        .query_row("SELECT commits FROM tickets WHERE id = 'RO-2'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, r#"["c8c5678","plexus-core@0bb28b2","090cdec"]"#);

    let store = Store::open(&path).unwrap();
    let got = store.get("RO-2").await.unwrap();
    assert_eq!(got, linked_at, "the acknowledged ticket, whole, after a reopen");
    assert_eq!(store.list(None, None, None).await.unwrap()[1], got);
    let page = store.page(PageQuery::default()).unwrap();
    assert_eq!(page.tickets[0].commits, Vec::<String>::new(), "never linked: []");
    assert_eq!(page.tickets[1], TicketSummary::from(got.clone()), "the summary row carries them");
    assert_eq!(store.find_page("RO-2", None, None).unwrap().tickets[0].commits, got.commits);
    let json = serde_json::to_value(&page.tickets[0]).unwrap();
    assert_eq!(json["commits"], serde_json::json!([]), "on the wire as [], never missing");
}

/// The schema as it was before SDLC-9 (`499e3a0`): no `commits` column.
const SCHEMA_BEFORE: &str = "
    CREATE TABLE tickets (
        id TEXT PRIMARY KEY, epic TEXT NOT NULL, seq INTEGER NOT NULL,
        title TEXT NOT NULL, status TEXT NOT NULL, kind TEXT NOT NULL,
        severity TEXT, blocked_by TEXT NOT NULL, unlocks TEXT NOT NULL,
        parent TEXT, body TEXT NOT NULL,
        created_unix INTEGER NOT NULL, updated_unix INTEGER NOT NULL,
        UNIQUE (epic, seq)
    );
    INSERT INTO tickets VALUES
        ('OLD-1', 'OLD', 1, 'root', 'Complete', 'scope', NULL, '[]', '[\"OLD-2\"]', NULL, 'b1', 1, 2),
        ('OLD-2', 'OLD', 2, 'child', 'In Review', 'build', 'High', '[\"OLD-1\"]', '[]', 'OLD-1', 'b2', 3, 4);
";

/// A database written before SDLC-9 — the live store's own shape, the
/// schema as of `499e3a0` — opens with every ticket at `commits: []`; the
/// column is in the file, so the next open adds nothing; and a link made
/// after it is as durable as on a new file.
#[tokio::test]
async fn a_database_from_before_the_column_opens_with_none_linked_durably() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("before.db");
    Connection::open(&path).unwrap().execute_batch(SCHEMA_BEFORE).unwrap();
    assert!(!db::has_column(&Connection::open(&path).unwrap(), "tickets", "commits").unwrap());

    {
        let store = Store::open(&path).unwrap();
        for t in store.list(None, None, None).await.unwrap() {
            assert_eq!(t.commits, Vec::<String>::new(), "{}", t.id);
        }
        let old2 = store.get("OLD-2").await.unwrap();
        assert_eq!(
            (old2.status, old2.parent.as_deref(), old2.body.as_str(), old2.updated_unix),
            (Status::InReview, Some("OLD-1"), "b2", 4),
            "nothing else read differently"
        );
        assert_eq!(store.get("OLD-1").await.unwrap().child_count, 1);
        store.link_commits("OLD-2", strings(&["77df9a0"])).await.unwrap();
    }
    let mut conn = Connection::open(&path).unwrap();
    assert!(db::has_column(&conn, "tickets", "commits").unwrap(), "the column is in the file");
    assert!(!db::migrate_add_commits(&mut conn).unwrap(), "and the next open adds nothing");
    let stored: Vec<String> = conn
        .prepare("SELECT commits FROM tickets ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(stored, ["[]", r#"["77df9a0"]"#]);
    drop(conn);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("OLD-2").await.unwrap().commits, ["77df9a0"]);
    assert_eq!(store.get("OLD-1").await.unwrap().commits, Vec::<String>::new());
}
