//! NOTE-2: the notes store — a note kept dated and untitled, listed newest
//! first, filtered and paged; an upgrade's one write (the ticket and the
//! note's pointer together) and its refusals (a note that is none or is a
//! ticket already, a ticket `create` refuses — the note left open, no id
//! spent); and the notes as a file keeps them, across a reopen.

use super::*;
use crate::notes::{Note, NoteState, Problem};

fn ticket(epic: &str, kind: Kind, title: &str) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        body: String::new(),
    }
}

fn ids(page: &NotePage) -> Vec<u64> {
    page.notes.iter().map(|n| n.id.get()).collect()
}

fn all(store: &Store, q: NoteQuery) -> Vec<u64> {
    ids(&store.notes(&q, None, None).unwrap())
}

/// The ticket a note `n` asks for: as `notes.upgrade` asks, its title and
/// body the note's.
fn from_note(store: &Store, id: u64, epic: &str) -> NewTicket {
    let (title, body) = Store::note_ticket_text(&store.note(id).unwrap());
    NewTicket { title, body, ..ticket(epic, Kind::Build, "") }
}

#[tokio::test]
async fn a_note_is_kept_dated_untitled_and_listed_newest_first() {
    let store = Store::new();
    let a = store.add_note_as("the palette should remember its query".into(), None, None).await.unwrap();
    let b = store.add_note_as("Watch drops events".into(), Some(" ada ".into()), Some("idp:ada".into())).await.unwrap();
    let c = store.add_note_as("  indented, as written  ".into(), None, None).await.unwrap();
    assert_eq!((a.id.get(), b.id.get(), c.id.get()), (1, 2, 3));
    assert_eq!((a.author.as_str(), b.author.as_str(), b.principal.as_deref()), ("owner", "ada", Some("idp:ada")));
    assert_eq!(c.body, "  indented, as written  ", "the body as written");
    assert!(a.at > 0 && a.ticket.is_none() && a.upgraded_at.is_none() && a.state() == NoteState::Open);
    assert_eq!(store.note(2).unwrap(), b);

    assert_eq!(all(&store, NoteQuery::default()), [3, 2, 1]);
    assert_eq!(all(&store, NoteQuery { author: Some("ada".into()), ..Default::default() }), [2]);
    assert_eq!(all(&store, NoteQuery { text: Some("WATCH".into()), ..Default::default() }), [2], "any ASCII case");
    assert_eq!(all(&store, NoteQuery { author: Some("  ".into()), ..Default::default() }), [3, 2, 1], "blank is no filter");
    assert_eq!(all(&store, NoteQuery { state: Some(NoteState::Upgraded), ..Default::default() }), Vec::<u64>::new());

    // Pages: newest first, the cursor an id.
    let first = store.notes(&NoteQuery::default(), None, Some(2)).unwrap();
    assert_eq!((ids(&first), first.next), (vec![3, 2], Some(2)));
    let rest = store.notes(&NoteQuery::default(), first.next, Some(2)).unwrap();
    assert_eq!((ids(&rest), rest.next), (vec![1], None));
    assert!(matches!(store.notes(&NoteQuery::default(), Some(9), None), Err(StoreError::NoteCursor(9))));

    // Refused: nothing kept, no id spent.
    assert!(matches!(store.add_note_as("  \n ".into(), None, None).await, Err(StoreError::NoteRefused(Problem::EmptyBody))));
    assert!(matches!(store.add_note_as("x".into(), Some("".into()), None).await, Err(StoreError::NoteRefused(Problem::BlankAuthor))));
    assert_eq!(store.add_note_as("next".into(), None, None).await.unwrap().id, 4);
    assert!(matches!(store.note(9), Err(StoreError::NoteNotFound { id: 9, newest: Some(4) })));
    assert!(matches!(Store::new().note(1), Err(StoreError::NoteNotFound { id: 1, newest: None })));
}

#[tokio::test]
async fn an_upgrade_makes_the_ticket_and_marks_the_note_in_one_write() {
    let store = Store::new();
    store.create(SYS, ticket("SR", Kind::Scope, "the root")).await.unwrap();
    let note = store.add_note_as("## watch drops **events**\nseen twice".into(), None, None).await.unwrap();
    let mut watch = store.subscribe();

    let (made, marked) = store.upgrade_note(SYS, note.id.get(), from_note(&store, note.id.get(), "sr")).await.unwrap();
    assert_eq!((made.id.as_str(), made.title.as_str(), made.parent.as_deref()), ("SR-2", "watch drops events", Some("SR-1")));
    assert!(made.body.starts_with("## watch drops **events**\nseen twice\n\nNoted ") && made.body.ends_with(" — note 1."), "{}", made.body);
    assert_eq!(marked, Note { ticket: Some("SR-2".into()), upgraded_at: Some(made.created_unix), ..note.clone() });
    assert_eq!(store.note(1).unwrap(), marked, "the cache holds the mark");
    assert_eq!(store.get(SYS, "SR-2").await.unwrap().title, made.title);
    assert!(matches!(watch.recv().await.unwrap(), Change::Created { ticket } if ticket.id == "SR-2"), "the birth is a create's");
    assert_eq!(all(&store, NoteQuery { state: Some(NoteState::Upgraded), ..Default::default() }), [1]);
    assert_eq!(all(&store, NoteQuery { state: Some(NoteState::Open), ..Default::default() }), Vec::<u64>::new());

    // Already a ticket: refused, nothing made.
    match store.upgrade_note(SYS, 1, from_note(&store, 1, "SR")).await {
        Err(StoreError::NoteUpgraded { id: 1, ticket }) => assert_eq!(ticket, "SR-2"),
        other => panic!("{other:?}"),
    }
    // A note that is none: refused, nothing made.
    assert!(matches!(store.upgrade_note(SYS, 7, ticket("SR", Kind::Build, "x")).await, Err(StoreError::NoteNotFound { id: 7, .. })));

    // A ticket create refuses: the note stays open, no id is spent.
    let open = store.add_note_as("a second thought".into(), None, None).await.unwrap();
    assert!(matches!(store.upgrade_note(SYS, open.id.get(), from_note(&store, open.id.get(), "BAD EPIC")).await, Err(StoreError::InvalidEpic(_))));
    let parented = NewTicket { parent: Some("NOPE-1".into()), ..from_note(&store, open.id.get(), "SR") };
    assert!(store.upgrade_note(SYS, open.id.get(), parented).await.is_err(), "a parent that is none");
    assert_eq!(store.note(open.id.get()).unwrap(), open, "still open, untouched");
    assert_eq!(store.create(SYS, ticket("SR", Kind::Build, "next")).await.unwrap().id, "SR-3", "no id was spent");
}

/// A title given wins; a note with no words is titled by when it was taken.
#[tokio::test]
async fn a_note_with_no_words_is_titled_by_its_date() {
    let store = Store::new();
    let n = store.add_note_as("---\n***".into(), None, None).await.unwrap();
    let (title, body) = Store::note_ticket_text(&n);
    assert!(title.starts_with("Note 1 of ") && title.ends_with(" UTC"), "{title}");
    assert!(body.starts_with("---\n***\n\nNoted "), "{body}");
}

fn notes_table(path: &Path) -> bool {
    let conn = Connection::open(path).unwrap();
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'notes'", [], |r| r.get::<_, i64>(0)).unwrap() == 1
}

#[tokio::test]
async fn notes_and_their_tickets_survive_a_reopen_and_an_older_file_gains_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.db");
    let (kept, marked) = {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket("SR", Kind::Scope, "the root")).await.unwrap();
        let kept = store.add_note_as("kept as written".into(), Some("ada".into()), None).await.unwrap();
        let other = store.add_note_as("made a ticket".into(), None, None).await.unwrap();
        let (_, marked) = store.upgrade_note(SYS, other.id.get(), from_note(&store, other.id.get(), "SR")).await.unwrap();
        (kept, marked)
    };
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.note(kept.id.get()).unwrap(), kept);
        assert_eq!(store.note(marked.id.get()).unwrap(), marked);
        assert_eq!(store.get(SYS, "SR-2").await.unwrap().title, "made a ticket");
        assert_eq!(store.add_note_as("after".into(), None, None).await.unwrap().id, 3, "ids go on from the file");
    }
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }), "notes are no derived state");

    // A file written before NOTE-2 has no table: the open makes it, empty.
    Connection::open(&path).unwrap().execute_batch("DROP TABLE notes").unwrap();
    assert!(!notes_table(&path));
    let store = Store::open(&path).unwrap();
    assert!(notes_table(&path));
    assert_eq!(all(&store, NoteQuery::default()), Vec::<u64>::new());
    assert_eq!(store.get(SYS, "SR-2").await.unwrap().title, "made a ticket", "the tickets as they were");
}
