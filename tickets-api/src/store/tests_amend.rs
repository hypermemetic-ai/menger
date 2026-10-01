//! MNG-16: `amend` — a ticket's title and body replaced after birth, through
//! the write-through path every other write takes: durable, then the
//! cache, then one `Amended`. Every row of the ticket's table, and the two
//! things it must not do: change anything else about the ticket, or say
//! anything when nothing changed.

use super::*;

fn ticket(epic: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: "the title it was born with".into(),
        kind,
        severity: Some(Severity::High),
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(Into::into),
        body: "## Problem\n\nthe body it was born with".into(),
    }
}

fn amended(change: Change) -> (Ticket, Vec<String>) {
    match change {
        Change::Amended { ticket, changed } => (ticket, changed),
        other => panic!("expected Amended, got {other:?}"),
    }
}

/// A body nothing normalizes: CRLF and LF, tabs, trailing spaces, a
/// mermaid fence, frontmatter, astral-plane characters, and a final line
/// with no newline.
const AWKWARD: &str = "---\nid: AM-2\nstatus: Pending\n---\r\n\n## Divergence notes\n\n\
    - 2026-10-01: ünïcode ✓ 𝔘 🧪\t(tab)   \r\n\
    ```mermaid\ngraph LR\n  A --> B\n```\n\ntrailing spaces   \nno final newline";

/// Criterion 1 at the store: a body round-trips byte-exact through `get`
/// and through the file — reopened, it is the same bytes.
#[tokio::test]
async fn a_body_round_trips_byte_exact_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("amend.db");
    let after = {
        let store = Store::open(&path).unwrap();
        store.create(ticket("AM", Kind::Scope, None, &[])).await.unwrap();
        store.create(ticket("AM", Kind::Build, None, &[])).await.unwrap();
        let (t, changed) = amended(store.amend("am-2", None, Some(AWKWARD.into())).await.unwrap());
        assert_eq!(changed, ["body"]);
        assert_eq!(t.body, AWKWARD);
        assert_eq!(store.get("AM-2").unwrap().body.as_bytes(), AWKWARD.as_bytes());
        t
    };
    let store = Store::open(&path).unwrap();
    let got = store.get("AM-2").unwrap();
    assert_eq!(got.body.as_bytes(), AWKWARD.as_bytes(), "the file holds the same bytes");
    assert_eq!(got, after, "and the whole ticket as acknowledged");
}

/// The table, row by row: body, title, both in one write, neither, equal
/// values, a missing id, a blank title. Each changing amend is exactly one
/// `Amended` on the channel, in acknowledgment order, and no
/// `group_changed`; a no-op and a refusal put nothing on it.
#[tokio::test]
async fn every_row_of_the_table() {
    let store = Store::new();
    store.create(ticket("AM", Kind::Scope, None, &[])).await.unwrap(); // AM-1
    let born = store.create(ticket("AM", Kind::Build, None, &[])).await.unwrap(); // AM-2
    let mut rx = store.subscribe();
    let next = |rx: &mut broadcast::Receiver<Change>| match rx.try_recv() {
        Ok(Change::Amended { ticket, changed }) => (ticket, changed),
        other => panic!("expected one Amended, got {other:?}"),
    };

    // the body
    let (t, changed) = amended(store.amend("AM-2", None, Some("a new body".into())).await.unwrap());
    assert_eq!(changed, ["body"]);
    assert_eq!((t.title.as_str(), t.body.as_str()), (born.title.as_str(), "a new body"));
    assert_eq!(next(&mut rx), (t, changed));
    assert!(rx.try_recv().is_err(), "one event, and no group moved");

    // the title
    let (t, changed) = amended(store.amend("AM-2", Some("a new title".into()), None).await.unwrap());
    assert_eq!(changed, ["title"]);
    assert_eq!((t.title.as_str(), t.body.as_str()), ("a new title", "a new body"));
    assert_eq!(next(&mut rx), (t, changed));

    // both, in one write — and `changed` names both, title first
    let (t, changed) =
        amended(store.amend("AM-2", Some("third title".into()), Some("third body".into())).await.unwrap());
    assert_eq!(changed, ["title", "body"]);
    assert_eq!((t.title.as_str(), t.body.as_str()), ("third title", "third body"));
    assert_eq!(next(&mut rx), (t.clone(), changed));
    assert!(rx.try_recv().is_err(), "both fields, one event");
    assert_eq!(store.get("AM-2").unwrap(), t);

    // neither given; the values already there; one equal and one not
    for (title, body) in [
        (None, None),
        (Some("third title"), Some("third body")),
        (Some("third title"), None),
        (None, Some("third body")),
    ] {
        let (got, changed) =
            amended(store.amend("AM-2", title.map(Into::into), body.map(Into::into)).await.unwrap());
        assert!(changed.is_empty(), "{title:?} {body:?}: {changed:?}");
        assert_eq!(got, t, "the ticket as it is, unwritten");
        assert!(rx.try_recv().is_err(), "{title:?} {body:?}: a no-op broadcasts nothing");
    }
    let (_, changed) =
        amended(store.amend("AM-2", Some("third title".into()), Some("fourth body".into())).await.unwrap());
    assert_eq!(changed, ["body"], "only the value that differs");
    next(&mut rx);

    // a missing id; a blank title — nothing written, nothing said
    assert!(matches!(store.amend("AM-9", None, Some("x".into())).await, Err(StoreError::NotFound(id)) if id == "AM-9"));
    for blank in ["", "   ", "\t\n"] {
        match store.amend("AM-2", Some(blank.into()), Some("a body that must not land".into())).await {
            Err(e @ StoreError::EmptyTitle { .. }) => {
                assert!(e.to_string().contains("AM-2") && e.to_string().contains("empty"), "{e}")
            }
            other => panic!("{blank:?}: expected EmptyTitle, got {other:?}"),
        }
    }
    assert!(rx.try_recv().is_err());
    assert_eq!(store.get("AM-2").unwrap().body, "fourth body", "the refused amend wrote nothing");
}

/// "Nothing else about the ticket changes": on a ticket with every edge —
/// a parent, a child, a blocker, a ticket it unlocks, a non-default
/// status — an amend of both fields moves the title, the body and
/// `updated_unix`, and nothing else on it or on any other ticket; the
/// roll-ups and the id indexes are what they were.
#[tokio::test]
async fn an_amend_changes_the_title_body_and_updated_unix_and_nothing_else() {
    let store = Store::new();
    store.create(ticket("ED", Kind::Scope, None, &[])).await.unwrap(); // ED-1
    store.create(ticket("ED", Kind::Build, None, &[])).await.unwrap(); // ED-2
    store.create(ticket("ED", Kind::Gap, Some("ED-1"), &["ED-2", "ED-9"])).await.unwrap(); // ED-3
    store.create(ticket("ED", Kind::Spike, Some("ED-3"), &["ED-3"])).await.unwrap(); // ED-4
    store.set_status("ED-3", Status::Coding).await.unwrap();
    let before: Vec<Ticket> = store.list(None, None, None);
    let groups = store.groups();
    let extents = store.extents();
    let created_before = chrono::Utc::now().timestamp();

    let (t, _) = amended(
        store
            .amend("ED-3", Some("retitled".into()), Some("rewritten".into()))
            .await
            .unwrap(),
    );
    let was = before.iter().find(|b| b.id == "ED-3").unwrap();
    assert!(t.updated_unix >= created_before, "updated_unix moved to the amend's time");
    let mut expect = was.clone();
    expect.title = "retitled".into();
    expect.body = "rewritten".into();
    expect.updated_unix = t.updated_unix;
    assert_eq!(t, expect, "status, kind, severity, edges, parent, children, id, seq, created_unix: as they were");

    let after = store.list(None, None, None);
    for (b, a) in before.iter().zip(&after) {
        if a.id != "ED-3" {
            assert_eq!(a, b, "{} untouched", a.id);
        }
    }
    assert_eq!(store.groups(), groups, "no roll-up moved");
    assert_eq!(store.extents(), extents, "no sequence moved");
    // The id index kept ED-3's forward reference: ED-9, when born, unlocks it.
    for _ in 5..=9 {
        store.create(ticket("ED", Kind::Build, None, &[])).await.unwrap();
    }
    assert_eq!(store.get("ED-9").unwrap().unlocks, ["ED-3"]);
}
