//! WAVE-10: path addresses — `<SHORTCODE>/<path>` names the one ticket of
//! that shortcode holding `path`. Only `_MENGER/…`, only on a doc, only
//! directly under the shortcode's scope root, unique in the shortcode;
//! `get` resolves an address (the shortcode case-insensitive); and a file
//! from before WAVE-10 gains the column on its first open, every row kept.

use super::stored::{self, State};
use super::*;

fn new(epic: &str, kind: Kind, parent: Option<&str>, path: Option<&str>) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        path: path.map(Into::into),
        body: format!("the body of a {} in {epic}", kind.as_str()),
    }
}

fn board(epic: &str) -> NewTicket {
    new(epic, Kind::Doc, None, Some("_MENGER/board"))
}

/// A platform: PLX-1 its scope root, PLX-2 a build under it, and SUB a
/// sub-shortcode hosted under PLX-1.
async fn platform() -> Store {
    let store = Store::new();
    store.create(new("PLX", Kind::Scope, None, None)).await.unwrap();
    store.create(new("PLX", Kind::Build, None, None)).await.unwrap();
    store.create(new("SUB", Kind::Scope, Some("PLX-1"), None)).await.unwrap();
    store
}

/// Acceptance 1: a `_MENGER/` doc is born under the scope root — the
/// parent omitted (it defaults there) or named — with its path on the
/// ticket, the row and the `created` event; it keeps an `EPIC-N` id, and the
/// next id after it is the next one.
#[tokio::test]
async fn a_menger_doc_is_born_under_the_scope_root_with_its_path() {
    let store = platform().await;
    let mut rx = store.subscribe();
    let t = store.create(board("PLX")).await.unwrap();
    assert_eq!(t.id, "PLX-3", "an address is not an id");
    assert_eq!((t.kind, t.status), (Kind::Doc, Status::Doc));
    assert_eq!(t.parent.as_deref(), Some("PLX-1"), "the scope root, by default");
    assert_eq!(t.path.as_deref(), Some("_MENGER/board"));
    match rx.recv().await.unwrap() {
        Change::Created { ticket } => assert_eq!(ticket.path.as_deref(), Some("_MENGER/board")),
        other => panic!("expected Created, got {other:?}"),
    }
    let row = store.page(PageQuery { epic: Some("PLX"), ..Default::default() }).unwrap();
    assert_eq!(row.tickets[2].path.as_deref(), Some("_MENGER/board"), "on the row");
    assert_eq!(row.tickets[1].path, None, "a ticket without one has none");

    // The root named as the parent, explicitly; and in a sub-shortcode, its
    // own root.
    let named = store.create(new("PLX", Kind::Doc, Some("plx-1"), Some("_MENGER/notes"))).await.unwrap();
    assert_eq!((named.id.as_str(), named.parent.as_deref()), ("PLX-4", Some("PLX-1")));
    let sub = store.create(board("SUB")).await.unwrap();
    assert_eq!((sub.id.as_str(), sub.parent.as_deref()), ("SUB-2", Some("SUB-1")));
    let next = store.create(new("PLX", Kind::Build, None, None)).await.unwrap();
    assert_eq!(next.id, "PLX-5", "ids go on as before");
}

/// Acceptance 1: `get` by address — the shortcode case-insensitive, the
/// path exact — is the ticket `get` by id answers; an address that names
/// nothing is `NotFound` naming the address as given; `get` by id is
/// untouched.
#[tokio::test]
async fn get_resolves_a_path_address_case_insensitively_in_the_shortcode() {
    let store = platform().await;
    let t = store.create(board("PLX")).await.unwrap();
    let by_id = store.get("PLX-3").await.unwrap();
    assert_eq!(by_id, t);
    for address in ["PLX/_MENGER/board", "plx/_MENGER/board", "Plx/_MENGER/board"] {
        assert_eq!(store.get(address).await.unwrap(), t, "{address}");
    }
    for missing in ["PLX/_MENGER/BOARD", "PLX/_MENGER/boards", "SUB/_MENGER/board", "NOPE/_MENGER/board", "PLX/board"] {
        match store.get(missing).await {
            Err(StoreError::NotFound(id)) => assert_eq!(id, missing, "names the address"),
            other => panic!("{missing}: expected NotFound, got {other:?}"),
        }
    }
    assert!(matches!(store.get("PLX-9").await, Err(StoreError::NotFound(_))));
    // Not an address: a slash with nothing on one side, or a shortcode
    // that is no shortcode.
    for odd in ["/_MENGER/board", "PLX/", "P-X/_MENGER/board"] {
        assert!(matches!(store.get(odd).await, Err(StoreError::NotFound(_))), "{odd}");
    }
}

/// Acceptance 1: a path already held in the shortcode is refused in-band,
/// naming the holder — nothing stored, no id spent; the same path in
/// another shortcode is another address.
#[tokio::test]
async fn a_path_taken_in_its_shortcode_is_refused_naming_the_holder() {
    let store = platform().await;
    store.create(board("PLX")).await.unwrap();
    let before = store.count();
    match store.create(board("plx")).await {
        Err(e @ StoreError::PathTaken { .. }) => {
            let StoreError::PathTaken { epic, path, holder } = &e else { unreachable!() };
            assert_eq!((epic.as_str(), path.as_str(), holder.as_str()), ("PLX", "_MENGER/board", "PLX-3"));
            let msg = e.to_string();
            assert!(msg.contains("PLX-3") && msg.contains("PLX/_MENGER/board"), "{msg}");
        }
        other => panic!("expected PathTaken, got {other:?}"),
    }
    assert_eq!(store.count(), before, "nothing stored");
    assert_eq!(store.create(new("PLX", Kind::Build, None, None)).await.unwrap().id, "PLX-4", "no id spent");
    assert_eq!(store.create(board("SUB")).await.unwrap().path.as_deref(), Some("_MENGER/board"));
}

/// Acceptance 1: what is refused, in-band, before anything is stored — a
/// path outside `_MENGER/`, `_MENGER/` alone, a path on work, a parent other
/// than the scope root, and a shortcode with no root to live under.
#[tokio::test]
async fn non_docs_wrong_parents_and_unreserved_paths_are_refused() {
    let store = platform().await;
    store.create(new("PLX", Kind::Doc, None, None)).await.unwrap(); // PLX-3, a plain doc
    let before = store.count();
    for path in ["board", "MENGER/board", "_menger/board", "_MENGER/", "_MENGER", " _MENGER/board"] {
        match store.create(new("PLX", Kind::Doc, None, Some(path))).await {
            Err(e @ StoreError::PathNotReserved { .. }) => assert!(e.to_string().contains("_MENGER/"), "{e}"),
            other => panic!("{path:?}: expected PathNotReserved, got {other:?}"),
        }
    }
    for kind in [Kind::Build, Kind::Scope, Kind::Spike, Kind::Gap, Kind::Execution] {
        match store.create(new("PLX", kind, None, Some("_MENGER/board"))).await {
            Err(e @ StoreError::PathOnWork { .. }) => assert!(e.to_string().contains("only a doc"), "{e}"),
            other => panic!("{kind:?}: expected PathOnWork, got {other:?}"),
        }
    }
    for parent in ["PLX-2", "PLX-3"] {
        match store.create(new("PLX", Kind::Doc, Some(parent), Some("_MENGER/board"))).await {
            Err(StoreError::PathParent { parent: p, root, .. }) => assert_eq!((p.as_str(), root.as_str()), (parent, "PLX-1")),
            other => panic!("{parent}: expected PathParent, got {other:?}"),
        }
    }
    // A shortcode's first ticket has no root above it to live under.
    match store.create(board("NEW")).await {
        Err(StoreError::PathNoRoot { epic, .. }) => assert_eq!(epic, "NEW"),
        other => panic!("expected PathNoRoot, got {other:?}"),
    }
    assert_eq!(store.count(), before, "nothing stored");
    assert!(store.extents().iter().all(|(e, _)| e != "NEW"), "no shortcode started");
}

/// A path survives a reopen — the column in the file, the address answered
/// from the cache it loads — and the file's own index holds it unique.
#[tokio::test]
async fn a_path_is_durable_and_unique_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("paths.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(new("PLX", Kind::Scope, None, None)).await.unwrap();
        store.create(board("PLX")).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("plx/_MENGER/board").await.unwrap().id, "PLX-2");
    assert_eq!(store.get("PLX-2").await.unwrap().path.as_deref(), Some("_MENGER/board"));
    drop(store);
    let conn = Connection::open(&path).unwrap();
    let dup = conn.execute("UPDATE tickets SET path = '_MENGER/board' WHERE id = 'PLX-1'", []);
    assert!(dup.is_err(), "the file refuses a second holder of a path in a shortcode");
}

/// Acceptance 1: a database from before WAVE-10 — this build's own file
/// with the column taken away, as prod's is — gains `path` on its first
/// open, keeps every row as it was (nothing rewritten, the stored derived
/// state still current: no rebuild), and the next open adds nothing.
#[tokio::test]
async fn an_older_database_gains_the_column_on_open_and_keeps_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("before.db");
    let before: Vec<Ticket> = {
        let store = Store::open(&path).unwrap();
        store.create(new("OLD", Kind::Scope, None, None)).await.unwrap();
        store.create(new("OLD", Kind::Build, None, None)).await.unwrap();
        store.create(new("OLD", Kind::Doc, None, None)).await.unwrap();
        store.set_status("OLD-2", Status::InReview).await.unwrap();
        store.link_commits("OLD-2", vec!["77df9a0".into()]).await.unwrap();
        store.list(None, None, None).await.unwrap()
    };
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP INDEX tickets_path; ALTER TABLE tickets DROP COLUMN path;").unwrap();
        assert!(!db::has_column(&conn, "tickets", "path").unwrap());
        assert_eq!(stored::state(&conn).unwrap(), State::Current, "prod's shape: derived state stored");
    }
    assert!(
        matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }),
        "--verify-derived reads a file from before the column, unmigrated"
    );
    {
        let store = Store::open(&path).unwrap();
        let after = store.list(None, None, None).await.unwrap();
        assert_eq!(after, before, "every row as it was, none with a path");
        assert!(after.iter().all(|t| t.path.is_none()));
        let t = store.create(board("OLD")).await.unwrap();
        assert_eq!((t.id.as_str(), t.parent.as_deref()), ("OLD-4", Some("OLD-1")));
    }
    let mut conn = Connection::open(&path).unwrap();
    assert!(db::has_column(&conn, "tickets", "path").unwrap(), "the column is in the file");
    assert!(!db::migrate_add_path(&mut conn).unwrap(), "and the next open adds nothing");
    assert_eq!(stored::state(&conn).unwrap(), State::Current);
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("OLD/_MENGER/board").await.unwrap().id, "OLD-4");
}

/// The address grammar: a shortcode, a slash, a path — the shortcode
/// upper-cased, the path as written, the first slash the split.
#[test]
fn a_path_address_is_a_shortcode_a_slash_and_a_path() {
    use crate::types::{is_menger_path, path_address};
    assert_eq!(path_address("PLEXUS/_MENGER/board"), Some(("PLEXUS".into(), "_MENGER/board")));
    assert_eq!(path_address("plexus/_MENGER/board"), Some(("PLEXUS".into(), "_MENGER/board")));
    assert_eq!(path_address("PLEXUS-35"), None);
    assert_eq!(path_address("/_MENGER/board"), None);
    assert_eq!(path_address("PLEXUS/"), None);
    assert_eq!(path_address("PLE-XUS/_MENGER/board"), None);
    assert!(is_menger_path("_MENGER/board"));
    assert!(is_menger_path("_MENGER/a/b"));
    assert!(!is_menger_path("_MENGER/"));
    assert!(!is_menger_path("_menger/board"));
    assert!(!is_menger_path("board"));
}
