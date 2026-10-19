//! COMMENT-8: replies — a test per row of the ticket the store answers: a
//! reply takes its root's anchor and names the root, a reply to a reply
//! answers the same root, a reply to a resolved root reopens it in the same
//! write (both heard, both durable), `list { thread }` is the root and its
//! replies while the default list carries replies inline, a reply is seen by
//! exactly who sees its root, and a refusal keeps nothing.

use super::tests_reach::viewer_of;
use super::*;
use crate::comments::{Anchor, Comment, CommentState};

fn ticket(epic: &str, kind: Kind) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: String::new(),
    }
}

fn anchor(ticket: &str) -> Anchor {
    Anchor {
        route: format!("#/ticket/{ticket}"),
        path: "conversation".into(),
        label: "Conversation".into(),
        excerpt: String::new(),
        ticket: Some(ticket.into()),
        version: None,
    }
}

/// OPEN-1, OPEN-2 and SECRET-1, SECRET-2: two shortcodes, one hidden from
/// the viewer.
async fn tickets(store: &Store) {
    for (epic, kind) in [("OPEN", Kind::Scope), ("OPEN", Kind::Build), ("SECRET", Kind::Scope), ("SECRET", Kind::Build)] {
        store.create(SYS, ticket(epic, kind)).await.unwrap();
    }
}

fn thread(store: &Store, id: u64) -> Vec<u64> {
    let q = CommentQuery { thread: Some(id), ..CommentQuery::default() };
    store.comments(SYS, &q, None, None).unwrap().comments.iter().map(|c| c.id.get()).collect()
}

/// Rows 1 and 6: the reply is on its root's anchor (version included) and
/// names it; a reply to that reply names the root too; `watch` hears each
/// as `added`; both are read back after a restart.
#[tokio::test]
async fn a_reply_takes_its_roots_anchor_and_a_reply_to_a_reply_answers_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("threads.db");
    let (root, reply, again) = {
        let store = Store::open(&path).unwrap();
        tickets(&store).await;
        let root = store.add_comment(SYS, anchor("OPEN-2"), "why is this here?".into(), None).await.unwrap();
        let mut rx = store.subscribe_comments();
        let reply = store.reply_comment_as(SYS, root.id.get(), "because of the import".into(), Some("builder".into()), Some("key:54a9".into())).await.unwrap();
        assert_eq!(reply.reply_to, Some(root.id));
        assert_eq!(reply.anchor, root.anchor, "the root's anchor, its version too");
        assert_eq!((reply.author.as_str(), reply.principal.as_deref(), reply.state), ("builder", Some("key:54a9"), CommentState::Open));
        assert_eq!(rx.try_recv().unwrap(), CommentChange::Added(reply.clone()));
        let again = store.reply_comment_as(SYS, reply.id.get(), "and the move".into(), None, None).await.unwrap();
        assert_eq!(again.reply_to, Some(root.id), "a reply to a reply is a reply to its root");
        assert_eq!(rx.try_recv().unwrap(), CommentChange::Added(again.clone()));
        assert!(rx.try_recv().is_err(), "an open root is not reopened");
        (root, reply, again)
    };
    let store = Store::open(&path).unwrap();
    assert_eq!(store.comment(SYS, reply.id.get()).ok(), Some(reply));
    assert_eq!(store.comment(SYS, again.id.get()).ok(), Some(again));
    assert_eq!(store.comment(SYS, root.id.get()).unwrap().reply_to, None);
}

/// Row 5: a reply to a resolved root reopens it, in the same write — heard
/// as `added` then `reopened`, and durable.
#[tokio::test]
async fn a_reply_to_a_resolved_comment_reopens_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reopen.db");
    let root = {
        let store = Store::open(&path).unwrap();
        tickets(&store).await;
        let root = store.add_comment(SYS, anchor("OPEN-2"), "done?".into(), None).await.unwrap();
        store.resolve_comment(SYS, root.id.get()).await.unwrap();
        let mut rx = store.subscribe_comments();
        let reply = store.reply_comment_as(SYS, root.id.get(), "not quite".into(), None, None).await.unwrap();
        let reopened = Comment { state: CommentState::Open, ..root.clone() };
        assert_eq!(rx.try_recv().unwrap(), CommentChange::Added(reply));
        assert_eq!(rx.try_recv().unwrap(), CommentChange::Reopened(reopened.clone()));
        assert_eq!(store.comment(SYS, root.id.get()).ok(), Some(reopened));
        root
    };
    let store = Store::open(&path).unwrap();
    assert_eq!(store.comment(SYS, root.id.get()).unwrap().state, CommentState::Open);
}

/// Row 3: `thread` is the root and its replies, named by either; the
/// default list carries every reply inline, each with `reply_to`.
#[tokio::test]
async fn list_thread_is_the_root_and_its_replies() {
    let store = Store::new();
    tickets(&store).await;
    let a = store.add_comment(SYS, anchor("OPEN-2"), "a".into(), None).await.unwrap().id.get();
    let b = store.add_comment(SYS, anchor("OPEN-2"), "b".into(), None).await.unwrap().id.get();
    let a1 = store.reply_comment_as(SYS, a, "a1".into(), None, None).await.unwrap().id.get();
    let b1 = store.reply_comment_as(SYS, b, "b1".into(), None, None).await.unwrap().id.get();
    let a2 = store.reply_comment_as(SYS, a1, "a2".into(), None, None).await.unwrap().id.get();
    assert_eq!(thread(&store, a), [a2, a1, a]);
    assert_eq!(thread(&store, a1), [a2, a1, a], "a reply's id names its root's thread");
    assert_eq!(thread(&store, b), [b1, b]);
    let all = store.comments(SYS, &CommentQuery::default(), None, None).unwrap().comments;
    let shape: Vec<(u64, Option<u64>)> = all.iter().map(|c| (c.id.get(), c.reply_to.map(|r| r.get()))).collect();
    assert_eq!(shape, [(a2, Some(a)), (b1, Some(b)), (a1, Some(a)), (b, None), (a, None)]);
}

/// Row 1: a reply is seen by exactly who sees its root — a viewer of OPEN
/// sees OPEN's thread and nothing of SECRET's, by list, by id or by thread.
#[tokio::test]
async fn a_reply_is_seen_by_who_sees_its_root() {
    let store = Store::new();
    tickets(&store).await;
    let open = store.add_comment(SYS, anchor("OPEN-2"), "open".into(), None).await.unwrap().id.get();
    let secret = store.add_comment(SYS, anchor("SECRET-2"), "secret".into(), None).await.unwrap().id.get();
    let open1 = store.reply_comment_as(SYS, open, "open1".into(), None, None).await.unwrap().id.get();
    let secret1 = store.reply_comment_as(SYS, secret, "secret1".into(), None, None).await.unwrap().id.get();
    let v = viewer_of(&["OPEN"]);
    let seen: Vec<u64> = store.comments(&v, &CommentQuery::default(), None, None).unwrap().comments.iter().map(|c| c.id.get()).collect();
    assert_eq!(seen, [open1, open]);
    assert!(store.comment(&v, open1).is_ok());
    assert!(matches!(store.comment(&v, secret1), Err(StoreError::CommentNotFound { .. })));
    let q = CommentQuery { thread: Some(secret), ..CommentQuery::default() };
    assert!(store.comments(&v, &q, None, None).unwrap().comments.is_empty());
}

/// Row 1's refusals: an id naming no comment and an empty body keep
/// nothing — no row, no id spent, no event.
#[tokio::test]
async fn a_refused_reply_keeps_nothing() {
    let store = Store::new();
    tickets(&store).await;
    let root = store.add_comment(SYS, anchor("OPEN-2"), "root".into(), None).await.unwrap().id.get();
    let mut rx = store.subscribe_comments();
    assert!(matches!(store.reply_comment_as(SYS, 99, "lost".into(), None, None).await, Err(StoreError::CommentNotFound { id: 99, .. })));
    assert!(matches!(store.reply_comment_as(SYS, root, "  ".into(), None, None).await, Err(StoreError::CommentRefused(_))));
    assert!(rx.try_recv().is_err());
    assert_eq!(store.reply_comment_as(SYS, root, "kept".into(), None, None).await.unwrap().id.get(), root + 1, "no id spent");
}
