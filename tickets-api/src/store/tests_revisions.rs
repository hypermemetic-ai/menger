//! TICKETS-7: every write that changes a ticket's own fields keeps exactly
//! one revision per ticket it changed, and none for a no-op; a cascade and
//! a bulk write one per ticket, each naming the cause; a ticket older than
//! revisions keeps its version before; a deleted ticket keeps its revisions
//! and an undelete is one; restore round-trips, and is refused with the
//! write rules' reasons; a body shared by revisions is stored once; reads
//! stop at the reach; `--verify-derived` holds them to the tickets.

use super::history::Act;
use super::*;
use crate::revisions::RevisionSource;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| (*b).into()).collect(),
        parent: parent.map(Into::into),
        body: format!("the body of {title}\nsecond line\n"),
    }
}

/// `R`: R-1 a scope, R-2 and R-3 builds under it, R-4 a build.
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, new("R", "root", Kind::Scope, None, &[])).await.unwrap();
    store.create(SYS, new("R", "two", Kind::Build, Some("R-1"), &[])).await.unwrap();
    store.create(SYS, new("R", "three", Kind::Build, Some("R-1"), &[])).await.unwrap();
    store.create(SYS, new("R", "four", Kind::Build, None, &[])).await.unwrap();
    store
}

async fn revs(store: &Store, id: &str) -> Vec<crate::revisions::Revision> {
    let mut page = store.revisions(SYS, id, None, Some(500)).await.unwrap().revisions;
    page.reverse();
    page
}

async fn count(store: &Store, id: &str) -> usize {
    revs(store, id).await.len()
}

fn bodies(store: &Store) -> i64 {
    store.db.lock().unwrap().query_row("SELECT count(*) FROM revision_bodies", [], |r| r.get(0)).unwrap()
}

fn drained(rx: &mut broadcast::Receiver<Change>) -> Vec<(String, u32, Vec<String>)> {
    let mut out = Vec::new();
    while let Ok(c) = rx.try_recv() {
        if let Change::Revised { ticket, rev, fields, .. } = c {
            out.push((ticket.to_string(), rev, fields));
        }
    }
    out
}

#[tokio::test]
async fn a_birth_is_the_first_revision() {
    let store = store().await;
    let r = revs(&store, "R-2").await;
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].rev, r[0].fields.clone()), (1, vec!["created".to_string()]));
    assert_eq!(r[0].added, 2);
    assert_eq!(r[0].body_hash, revisions::hash("the body of two\nsecond line\n"));
}

#[tokio::test]
async fn every_write_keeps_one_revision_per_ticket_it_changed_and_none_for_a_no_op() {
    let store = store().await;
    let mut rx = store.subscribe_revisions();
    // amend: title and body, one revision.
    store.amend(SYS.permit("R-2"), Some("two, renamed".into()), Some("a new body\nsecond line\n".into())).await.unwrap();
    assert_eq!(drained(&mut rx), vec![("R-2".into(), 2, vec!["title".into(), "body".into()])]);
    let r = revs(&store, "R-2").await;
    assert_eq!((r[1].added, r[1].removed), (1, 1));
    // the same again: nothing.
    store.amend(SYS.permit("R-2"), Some("two, renamed".into()), None).await.unwrap();
    assert!(drained(&mut rx).is_empty());
    // set_status, set_kind, label, link, set_parent, update — each one.
    store.set_status(SYS.permit("R-2"), Status::Ready).await.unwrap();
    store.set_kind(SYS.permit("R-2"), Kind::Gap, None).await.unwrap();
    store.label(SYS.permit("R-2"), vec!["needs-you".into()], vec![]).await.unwrap();
    store.label(SYS.permit("R-2"), vec!["needs-you".into()], vec![]).await.unwrap();
    store.link_commits(SYS.permit("R-2"), vec!["77df9a0".into()]).await.unwrap();
    store.set_parent(SYS.moving("R-2", "R-3")).await.unwrap();
    store.update(SYS.updating("R-2"), Update { severity: Some(Some(Severity::High)), ..Update::default() }).await.unwrap();
    store.update(SYS.updating("R-2"), Update { block: vec!["R-4".into()], ..Update::default() }).await.unwrap();
    let said: Vec<Vec<String>> = drained(&mut rx).into_iter().map(|(t, _, f)| {
        assert_eq!(t, "R-2", "a blocker's unlocks and a parent's count keep no revision");
        f
    }).collect();
    assert_eq!(said, vec![vec!["status"], vec!["kind"], vec!["labels"], vec!["commits"], vec!["parent"], vec!["severity"], vec!["blocked_by"]]);
    assert_eq!(count(&store, "R-2").await, 9);
    assert_eq!(count(&store, "R-4").await, 1, "the blocker's own fields did not move");
    // one write of several aspects: one revision.
    store.update(SYS.updating("R-2"), Update { title: Some("t".into()), status: Some(Status::Coding), label: vec!["x".into()], ..Update::default() }).await.unwrap();
    assert_eq!(drained(&mut rx), vec![("R-2".into(), 10, vec!["title".into(), "status".into(), "labels".into()])]);
    // an archive's mark is no field of a revision.
    store.archive_by(Selection::Ids(vec!["R-4".into()]), Act::default(), false, |ids| Ok(SYS.permits(ids))).await.unwrap();
    assert!(drained(&mut rx).is_empty());
}

#[tokio::test]
async fn who_from_where_and_why() {
    let store = store().await;
    let act = Act { actor: "idp:ada".into(), name: Some("Ada".into()), source: crate::types::TransitionSource::App, message: Some("starting".into()), cause: None };
    store.set_status_by(SYS.permit("R-2"), Status::Coding, act.clone(), false).await.unwrap();
    let r = revs(&store, "R-2").await;
    let last = r.last().unwrap();
    assert_eq!((last.actor.as_str(), last.name.as_deref(), last.source, last.message.as_deref()), ("idp:ada", Some("Ada"), RevisionSource::App, Some("starting")));
    // A write with no act of its own: the caller's, as the activation sets it.
    revisions::acting(Act { actor: "key:bob".into(), ..Act::default() }, store.amend(SYS.permit("R-2"), Some("by bob".into()), None)).await.unwrap();
    assert_eq!(revs(&store, "R-2").await.last().unwrap().actor, "key:bob");
}

#[tokio::test]
async fn a_cancel_cascades_one_revision_per_ticket_each_naming_its_cause() {
    let store = store().await;
    let mut rx = store.subscribe_revisions();
    store.set_status(SYS.permit("R-1"), Status::Canceled).await.unwrap();
    let said = drained(&mut rx);
    let ids: Vec<&str> = said.iter().map(|(t, ..)| t.as_str()).collect();
    assert_eq!(&ids[..3], ["R-1", "R-2", "R-3"], "the cause first, then each it moved");
    for id in ["R-2", "R-3"] {
        let last = revs(&store, id).await.pop().unwrap();
        assert_eq!((last.source, last.cause.as_deref()), (RevisionSource::Cascade, Some("R-1")), "{id}");
    }
}

#[tokio::test]
async fn a_bulk_write_keeps_one_per_ticket_pointing_at_the_first() {
    let store = store().await;
    let changes = store.label_many(SYS.permits(["R-2", "R-3"]), vec!["stale".into()], vec![]).await.unwrap();
    assert_eq!(changes.len(), 2);
    let (two, three) = (revs(&store, "R-2").await.pop().unwrap(), revs(&store, "R-3").await.pop().unwrap());
    assert_eq!((two.cause, three.cause.as_deref()), (None, Some("R-2")));
}

#[tokio::test]
async fn a_ticket_older_than_revisions_keeps_its_version_before() {
    let store = store().await;
    store.db.lock().unwrap().execute_batch("DELETE FROM revisions; DELETE FROM revision_bodies;").unwrap();
    // A write that changes nothing of its own keeps nothing.
    store.label(SYS.permit("R-2"), vec![], vec!["absent".into()]).await.unwrap();
    store.update(SYS.updating("R-3"), Update { block: vec!["R-2".into()], ..Update::default() }).await.unwrap();
    assert_eq!(count(&store, "R-2").await, 0, "its unlocks moved, nothing of its own");
    store.amend(SYS.permit("R-2"), None, Some("rewritten\n".into())).await.unwrap();
    let r = revs(&store, "R-2").await;
    assert_eq!(r.len(), 2);
    assert_eq!((r[0].rev, r[0].source, r[0].fields.is_empty()), (1, RevisionSource::Baseline, true));
    let (_, before) = store.revision_body(SYS, "R-2", 1).await.unwrap();
    assert_eq!(before, "the body of two\nsecond line\n");
    assert_eq!((r[1].fields.clone(), r[1].added, r[1].removed), (vec!["body".to_string()], 1, 2));
}

#[tokio::test]
async fn a_body_shared_by_revisions_is_stored_once() {
    let store = store().await;
    let before = bodies(&store);
    for l in ["a", "b", "c"] {
        store.label(SYS.permit("R-2"), vec![l.into()], vec![]).await.unwrap();
    }
    assert_eq!(bodies(&store), before, "a label change costs a row, not a body");
    store.amend(SYS.permit("R-2"), None, Some("x".into())).await.unwrap();
    store.amend(SYS.permit("R-2"), None, Some("the body of two\nsecond line\n".into())).await.unwrap();
    assert_eq!(bodies(&store), before + 1, "the old body came back: kept once");
    let r = revs(&store, "R-2").await;
    assert_eq!(r.first().unwrap().body_hash, r.last().unwrap().body_hash);
}

#[tokio::test]
async fn a_revision_is_the_ticket_as_it_was_and_a_diff_says_what_moved() {
    let store = store().await;
    store.amend(SYS.permit("R-2"), Some("renamed".into()), Some("---\ntheme: ocean\n---\nthe body of two\nsecond LINE\n".into())).await.unwrap();
    store.label(SYS.permit("R-2"), vec!["x".into()], vec![]).await.unwrap();
    let (rev, t) = store.revision(SYS, "R-2", 1).await.unwrap();
    assert_eq!((rev.rev, t.title.as_str(), t.body.as_str(), t.labels.len()), (1, "two", "the body of two\nsecond line\n", 0));
    assert_eq!(t.parent.as_deref(), Some("R-1"));
    let d = store.revision_diff(SYS, "R-2", Some(1), None, DiffView::Source).await.unwrap();
    assert_eq!((d.from, d.to), (1, 3));
    let fields: Vec<&str> = d.fields.iter().map(|f| f.field.as_str()).collect();
    assert_eq!(fields, ["title", "body", "labels"]);
    assert_eq!(d.frontmatter.len(), 1);
    assert_eq!((d.frontmatter[0].field.as_str(), d.frontmatter[0].from.as_deref(), d.frontmatter[0].to.as_deref()), ("theme", None, Some("ocean")));
    let body = d.body.unwrap();
    assert_eq!((body.added, body.removed), (4, 1));
    let text = store.revision_diff(SYS, "R-2", Some(1), Some(2), DiffView::Text).await.unwrap().body.unwrap();
    assert_eq!((text.added, text.removed), (1, 1), "the frontmatter is no text a reader sees");
    assert!(matches!(store.revision(SYS, "R-2", 9).await, Err(StoreError::Invalid(_))));
}

#[tokio::test]
async fn restore_round_trips_and_is_itself_a_revision() {
    let store = store().await;
    store.amend(SYS.permit("R-2"), Some("renamed".into()), Some("new".into())).await.unwrap();
    store.label(SYS.permit("R-2"), vec!["x".into()], vec![]).await.unwrap();
    store.set_status(SYS.permit("R-2"), Status::Ready).await.unwrap();
    let r = store.restoring(SYS, "R-2", 1, None).await.unwrap();
    assert_eq!(r.fields, ["title", "body", "status", "labels"]);
    store.update(SYS.updating("R-2"), r.update).await.unwrap();
    let (_, was) = store.revision(SYS, "R-2", 1).await.unwrap();
    let now = store.get(SYS, "R-2").await.unwrap();
    assert_eq!((now.title, now.body, now.status, now.labels), (was.title, was.body, was.status, was.labels));
    assert_eq!(count(&store, "R-2").await, 5, "the restore is a new revision");
    // Only the body.
    store.amend(SYS.permit("R-2"), Some("again".into()), Some("again".into())).await.unwrap();
    let r = store.restoring(SYS, "R-2", 2, Some(vec!["body".into()])).await.unwrap();
    assert_eq!(r.fields, ["body"]);
    assert_eq!(r.update.body.as_deref(), Some("new"));
    // Nothing differs: nothing to write.
    store.update(SYS.updating("R-2"), r.update).await.unwrap();
    assert!(store.restoring(SYS, "R-2", 2, Some(vec!["body".into()])).await.unwrap().fields.is_empty());
}

#[tokio::test]
async fn restore_is_refused_where_the_write_rules_refuse() {
    let store = store().await;
    // R-4 blocks on R-3; then R-3 blocks on R-4 — restoring R-4's first
    // revision is fine, but a cycle is refused by update's rules.
    store.update(SYS.updating("R-3"), Update { block: vec!["R-4".into()], ..Update::default() }).await.unwrap();
    store.update(SYS.updating("R-4"), Update { title: Some("t".into()), ..Update::default() }).await.unwrap();
    store.update(SYS.updating("R-3"), Update { unblock: vec!["R-4".into()], ..Update::default() }).await.unwrap();
    store.update(SYS.updating("R-4"), Update { block: vec!["R-3".into()], ..Update::default() }).await.unwrap();
    // R-3 at its revision 2 waited on R-4, which now waits on R-3.
    let r = store.restoring(SYS, "R-3", 2, Some(vec!["blocked_by".into()])).await.unwrap();
    let e = store.update(SYS.updating("R-3"), r.update).await.unwrap_err();
    assert!(e.to_string().contains("cycle"), "{e}");
    // A parent the revision did not have: a root moved under another.
    store.create(SYS, new("Q", "q root", Kind::Scope, None, &[])).await.unwrap();
    assert!(store.restoring(SYS, "Q-1", 1, Some(vec!["parent".into()])).await.unwrap().fields.is_empty(), "nothing differs");
    store.set_parent(SYS.moving("Q-1", "R-1")).await.unwrap();
    let e = store.restoring(SYS, "Q-1", 1, Some(vec!["parent".into()])).await.err().unwrap();
    assert!(e.to_string().contains("never cleared"), "{e}");
    // Commits are never restored.
    assert!(store.restoring(SYS, "R-4", 1, Some(vec!["commits".into()])).await.is_err());
}

#[tokio::test]
async fn a_deleted_ticket_keeps_its_revisions_and_an_undelete_is_one() {
    let store = store().await;
    store.amend(SYS.permit("R-4"), Some("before the delete".into()), None).await.unwrap();
    store.delete(SYS, Selection::Ids(vec!["R-4".into()]), Deletion::default(), Some("filed twice".into())).await.unwrap();
    let r = revs(&store, "R-4").await;
    assert_eq!(r.iter().map(|r| r.fields.clone()).collect::<Vec<_>>(), [vec!["created".to_string()], vec!["title".into()], vec!["deleted".into()]]);
    assert_eq!(r[2].message.as_deref(), Some("filed twice"));
    let (_, t) = store.revision(SYS, "R-4", 2).await.unwrap();
    assert_eq!(t.title, "before the delete");
    store.undelete(SYS, &["R-4".to_string()]).await.unwrap();
    let r = revs(&store, "R-4").await;
    assert_eq!(r.last().unwrap().fields, ["undeleted"]);
}

#[tokio::test]
async fn a_reshortcode_moves_the_revisions_with_their_ticket() {
    let store = store().await;
    store.create(SYS, new("Q", "q root", Kind::Scope, None, &[])).await.unwrap();
    store.amend(SYS.permit("R-4"), Some("moving".into()), None).await.unwrap();
    store.reshortcode(SYS, Reshortcode { ids: vec!["R-4".into()], to: "Q".into(), ..Reshortcode::default() }).await.unwrap();
    let r = revs(&store, "Q-2").await;
    assert!(r.len() >= 2, "{r:?}");
    assert_eq!(r[1].fields, ["title"]);
}

#[tokio::test]
async fn reads_stop_at_the_reach() {
    let store = store().await;
    store.create(SYS, new("HID", "hidden", Kind::Scope, None, &[])).await.unwrap();
    let viewer = tests_reach::viewer_of(&["R"]);
    assert!(store.revisions(&viewer, "R-2", None, None).await.is_ok());
    assert!(matches!(store.revisions(&viewer, "HID-1", None, None).await, Err(StoreError::NotFound(_))));
    assert!(matches!(store.revision(&viewer, "HID-1", 1).await, Err(StoreError::NotFound(_))));
}

#[tokio::test]
async fn paging_is_newest_first() {
    let store = store().await;
    for i in 0..5 {
        store.amend(SYS.permit("R-2"), Some(format!("t{i}")), None).await.unwrap();
    }
    let p = store.revisions(SYS, "R-2", None, Some(2)).await.unwrap();
    assert_eq!((p.count, p.latest, p.next), (6, 6, Some(5)));
    assert_eq!(p.revisions.iter().map(|r| r.rev).collect::<Vec<_>>(), [6, 5]);
    let p = store.revisions(SYS, "R-2", Some(2), Some(2)).await.unwrap();
    assert_eq!((p.revisions.len(), p.next), (1, None));
}

#[tokio::test]
async fn verify_holds_the_revisions_to_the_tickets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("revisions.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, new("V", "one", Kind::Scope, None, &[])).await.unwrap();
        store.amend(SYS.permit("V-1"), Some("two".into()), Some("b".into())).await.unwrap();
    }
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { revisions: Some(2), .. }));
    let conn = Connection::open(&path).unwrap();
    conn.execute("UPDATE tickets SET title = 'around the store' WHERE id = 'V-1'", []).unwrap();
    match Store::verify_derived(&path).unwrap() {
        Verdict::Differs(lines) => assert!(lines.iter().any(|l| l.contains("revisions of V-1")), "{lines:?}"),
        other => panic!("{other:?}"),
    }
}
