//! ACT-5: `delete` and `undelete` — a test per row of the ticket's table: a
//! ticket nothing points at gone from every read with a tombstone `get`
//! answers; children refused unless reparented or deleted; blockers refused
//! unless unblocked (the blocked ticket's history naming the delete); a
//! shortcode's root, a component's or a theme's owner and a live input
//! refused; many by ids or a filter, all or none; the number never reused;
//! the tombstone and the history kept, the body until the purge; `may` asked
//! of every ticket that goes; `deleted` on watch and `left` for a filter;
//! undelete putting back what the delete moved; and after each write the
//! derived state what a scan gives, `--verify-derived` consistent and a
//! reopened file the cache.

use super::tests_update::assert_kept;
use crate::address::ShortcodeId;
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
        body: format!("the body of {title}"),
    }
}

/// `M`: M-1 its root, M-2 … M-6 builds under it (M-3 waits on M-2), M-7 a
/// scope under the root with M-8, M-9 builds under it; `N`: N-1 its root,
/// N-2 a build.
async fn fixture(store: &Store) {
    store.create(new("M", "the root", Kind::Scope, None)).await.unwrap();
    for i in 2..=6 {
        let mut t = new("M", &format!("build {i}"), Kind::Build, None);
        if i == 3 {
            t.blocked_by = vec!["M-2".into()];
        }
        store.create(t).await.unwrap();
    }
    store.create(new("M", "an area", Kind::Execution, Some("M-1"))).await.unwrap();
    store.create(new("M", "eight", Kind::Build, Some("M-7"))).await.unwrap();
    store.create(new("M", "nine", Kind::Build, Some("M-7"))).await.unwrap();
    store.create(new("N", "n root", Kind::Scope, None)).await.unwrap();
    store.create(new("N", "n two", Kind::Build, None)).await.unwrap();
}

async fn store() -> Store {
    let store = Store::new();
    fixture(&store).await;
    store
}

fn ids(ids: &[&str]) -> Selection {
    Selection::Ids(ids.iter().map(|s| s.to_string()).collect())
}

fn how(children: Option<Children>, unblock: bool) -> Deletion {
    Deletion { children, unblock }
}

fn drained(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_recv() {
        out.push(change);
    }
    out
}

fn written(store: &Store) -> u64 {
    store.tickets.read().unwrap().written
}

fn exists(store: &Store, id: &str) -> bool {
    store.tickets.read().unwrap().get(id).is_some()
}

/// Each refused id with its reasons, joined.
fn refused(e: StoreError) -> BTreeMap<String, String> {
    match e {
        StoreError::RefusedMany { refusals } => {
            refusals.into_iter().map(|(id, why)| (id, why.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))).collect()
        }
        other => panic!("not a refusal of many: {other}"),
    }
}

/// The kind of each change, in order.
fn kinds(changes: &[Change]) -> Vec<&'static str> {
    changes
        .iter()
        .map(|c| match c {
            Change::ParentChanged { .. } => "parent_changed",
            Change::ChildrenChanged { .. } => "children_changed",
            Change::ChildRemoved { .. } => "child_removed",
            Change::BlockersChanged { .. } => "blockers_changed",
            Change::UnlocksRemoved { .. } => "unlocks_removed",
            Change::UnlocksChanged { .. } => "unlocks_changed",
            Change::Deleted { .. } => "deleted",
            Change::Undeleted { .. } => "undeleted",
            Change::GroupChanged { .. } => "group_changed",
            Change::PlacementsChanged { .. } => "placements_changed",
            Change::InputChanged { .. } => "input_changed",
            Change::ComponentChanged { .. } | Change::ComponentRemoved { .. } => "component",
            Change::CellsMoved { .. } => "cells_moved",
            Change::UnderMoved { .. } => "under_moved",
            _ => "other",
        })
        .collect()
}

#[tokio::test]
async fn a_ticket_nothing_points_at_is_gone_from_every_read() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    let before = written(&store);
    let lens = Lens::parse("kind build").unwrap();
    let builds = store.counts_text(&lens);
    let done = store.delete(ids(&["m-6"]), Deletion::default(), Some("filed twice".into())).await.unwrap();
    assert_eq!(written(&store), before + 1, "one write");
    assert_eq!(done.ids, ["M-6"]);
    let t = &done.tombstones[0];
    assert_eq!((t.id.as_str(), t.title.as_str(), t.by.as_str(), t.message.as_deref()), ("M-6", "build 6", "anonymous", Some("filed twice")));
    assert_eq!(t.restorable_until_unix, Some(t.at_unix + KEEP_BODY_FOR));
    assert!(matches!(store.get("M-6").await, Err(StoreError::NotFound(_))));
    assert_eq!(store.tombstone("m-6").as_ref(), Some(t), "get's tombstone");
    let listed = store.list(Some("M"), None, None).await.unwrap();
    assert!(listed.iter().all(|t| t.id != "M-6"), "off the list");
    assert!(store.find_page("build 6", None, None).unwrap().tickets.is_empty(), "off find");
    assert!(store.search("build", None, Some(50), None).await.unwrap().hits.iter().all(|h| h.summary.id != "M-6"), "off search");
    let after = store.counts_text(&lens);
    let m = |c: &[(String, u64)]| c.iter().find(|(e, _)| e == "M").map_or(0, |(_, n)| *n);
    assert_eq!(m(&after), m(&builds) - 1, "off every filter");
    assert!(store.ready_set(None).await.unwrap().iter().all(|t| t.id != "M-6"));
    assert_eq!(store.holes(&ShortcodeId::from("M"), 9), [6], "a hole the extents name");
    assert_eq!(store.tickets.read().unwrap().children.of("M-1").len(), 5, "off its parent's children");
    let sent = drained(&mut rx);
    let k = kinds(&sent);
    assert_eq!(&k[..2], ["child_removed", "deleted"], "{k:?}");
    assert!(k.contains(&"group_changed") && k.last() == Some(&"cells_moved"), "{k:?}");
    let Some(Change::CellsMoved { moves }) = sent.last() else { unreachable!() };
    assert_eq!(moves.moves.iter().map(|m| (m.shortcode.as_str(), m.seq, m.to)).collect::<Vec<_>>(), [("M", 6, cells::GONE)]);
    assert_kept(&store, "a delete");
}

#[tokio::test]
async fn a_filtered_watch_hears_the_deleted_leave() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    let mut w = store.watcher(Lens::parse("kind build").unwrap());
    let mut other = store.watcher(Lens::parse("kind scope").unwrap());
    store.delete(ids(&["M-5"]), Deletion::default(), None).await.unwrap();
    let moves = drained(&mut rx)
        .into_iter()
        .find_map(|c| match c {
            Change::CellsMoved { moves } => Some(moves),
            _ => None,
        })
        .unwrap();
    assert_eq!(w.membership(&store, &moves, None), [Membership::Left { id: "M-5".into(), shortcode: ShortcodeId::from("M"), seq: 5 }]);
    assert!(other.membership(&store, &moves, None).is_empty(), "never in it");
}

#[tokio::test]
async fn children_are_refused_unless_told_what_moves() {
    let store = store().await;
    let before = written(&store);
    let why = refused(store.delete(ids(&["M-7"]), Deletion::default(), None).await.unwrap_err());
    assert!(why["M-7"].contains("M-8, M-9") && why["M-7"].contains("children reparent (to M-1) or children delete"), "{why:?}");
    assert_eq!(written(&store), before, "nothing written");

    let mut rx = store.subscribe();
    let done = store.delete(ids(&["M-7"]), how(Some(Children::Reparent), false), None).await.unwrap();
    assert_eq!(done.ids, ["M-7"]);
    assert_eq!(done.tombstones[0].reparented, ["M-8", "M-9"]);
    for c in ["M-8", "M-9"] {
        assert_eq!(store.get(c).await.unwrap().parent.as_deref(), Some("M-1"), "{c} moved to its grandparent");
    }
    let k = kinds(&drained(&mut rx));
    assert_eq!(&k[..6], ["parent_changed", "children_changed", "parent_changed", "children_changed", "child_removed", "deleted"], "{k:?}");
    assert_kept(&store, "a reparenting delete");

    let store = self::store().await;
    let done = store.delete(ids(&["M-7"]), how(Some(Children::Delete), false), None).await.unwrap();
    assert_eq!(done.ids, ["M-7", "M-8", "M-9"], "the subtree, listed");
    assert!(!exists(&store, "M-8") && !exists(&store, "M-9"));
    assert_kept(&store, "a subtree's delete");
}

#[tokio::test]
async fn a_ticket_waited_on_is_refused_unless_unblocked() {
    let store = store().await;
    let why = refused(store.delete(ids(&["M-2"]), Deletion::default(), None).await.unwrap_err());
    assert!(why["M-2"].contains("M-3 waits on it — unblock"), "{why:?}");
    let mut rx = store.subscribe();
    store.delete(ids(&["M-2"]), how(None, true), Some("a mistake".into())).await.unwrap();
    let m3 = store.get("M-3").await.unwrap();
    assert!(m3.blocked_by.is_empty(), "the block dropped");
    let k = kinds(&drained(&mut rx));
    assert_eq!(&k[..3], ["blockers_changed", "child_removed", "deleted"], "{k:?}");
    let h = store.history("M-3").unwrap();
    assert_eq!(h.deletions.iter().map(|t| (t.id.as_str(), t.unblocked.clone())).collect::<Vec<_>>(), [("M-2", vec!["M-3".to_string()])]);
    assert_kept(&store, "an unblocking delete");
    // And a ticket that waited on it, deleted with it, needs no unblock.
    let store = self::store().await;
    store.delete(ids(&["M-2", "M-3"]), Deletion::default(), None).await.unwrap();
    assert_kept(&store, "a blocker and its blocked together");
}

#[tokio::test]
async fn a_blocker_of_the_deleted_loses_it_from_unlocks() {
    let store = store().await;
    store.delete(ids(&["M-3"]), Deletion::default(), None).await.unwrap();
    assert!(store.get("M-2").await.unwrap().unlocks.is_empty());
    assert_kept(&store, "a blocked ticket's delete");
}

#[tokio::test]
async fn a_shortcodes_root_is_refused_while_it_holds_others() {
    let store = store().await;
    let why = refused(store.delete(ids(&["N-1"]), Deletion::default(), None).await.unwrap_err());
    assert!(why["N-1"].contains("roots the shortcode N, which holds 1 other ticket"), "{why:?}");
    let done = store.delete(ids(&["N-1"]), how(Some(Children::Delete), false), None).await.unwrap();
    assert_eq!(done.ids, ["N-1", "N-2"]);
    assert!(store.groups().iter().all(|g| g.epic != "N"), "no group for an empty shortcode");
    let n = store.extents_holed().into_iter().find(|(e, _, _, _)| e == "N").map(|(e, _, l, h)| (e.as_str().to_string(), l, h));
    assert_eq!(n, Some(("N".to_string(), 2, vec![1, 2])), "its numbers given out, none held");
    // The number is never reused: N's next is N-3, and it roots N again.
    let refused = store.create(new("N", "a build first", Kind::Build, None)).await.unwrap_err();
    assert!(matches!(refused, StoreError::FirstMustBeScope { .. }), "{refused}");
    let born = store.create(new("N", "n again", Kind::Scope, None)).await.unwrap();
    assert_eq!(born.id, "N-3");
    assert_eq!(born.parent, None);
    let kid = store.create(new("N", "under it", Kind::Build, None)).await.unwrap();
    assert_eq!(kid.parent.as_deref(), Some("N-3"));
    assert_kept(&store, "a shortcode deleted and begun again");
}

#[tokio::test]
async fn the_number_is_never_reused() {
    let store = store().await;
    store.delete(ids(&["M-9"]), Deletion::default(), None).await.unwrap();
    let born = store.create(new("M", "next", Kind::Build, None)).await.unwrap();
    assert_eq!(born.id, "M-10", "after the deleted M-9");
    let refused = store.create(NewTicket { blocked_by: vec!["M-9".into()], ..new("M", "waits", Kind::Build, None) }).await.unwrap_err();
    assert!(refused.to_string().contains("can't wait on M-9: deleted"), "{refused}");
    let refused = store.update("M-4", Update { block: vec!["M-9".into()], ..Update::default() }).await.unwrap_err();
    assert!(refused.to_string().contains("can't wait on M-9: deleted"), "{refused}");
}

#[tokio::test]
async fn an_owner_or_a_live_input_is_refused() {
    let store = store().await;
    let body = "```component m-thing\n<template>x</template>\n```\n\n```theme dusk\n:root { --accent: #123456; }\n```\n";
    store.create(NewTicket { body: body.into(), ..new("M", "owner", Kind::Build, None) }).await.unwrap();
    let asks = "<m-input id=\"notes\" kind=\"text\"><m-question>Anything?</m-question></m-input>\n";
    store.create(NewTicket { body: asks.into(), ..new("M", "asks", Kind::Build, None) }).await.unwrap();
    let why = refused(store.delete(ids(&["M-10", "M-11"]), Deletion::default(), None).await.unwrap_err());
    assert!(why["M-10"].contains("the component <m-thing>") && why["M-10"].contains("the theme `dusk`"), "{why:?}");
    assert!(why["M-11"].contains("an input (notes) — close it first"), "{why:?}");
    store.close_input("M-11", "notes").await.unwrap();
    store.delete(ids(&["M-11"]), Deletion::default(), None).await.unwrap();
    assert!(store.inputs(None, Some("M"), None).unwrap().iter().all(|i| i.ticket != "M-11"), "its inputs out of every read");
    assert_kept(&store, "a closed input's ticket deleted");
}

#[tokio::test]
async fn many_are_all_or_none_by_ids_or_a_filter() {
    let store = store().await;
    let before = written(&store);
    let why = refused(store.delete(ids(&["M-4", "M-7", "NOPE-1"]), Deletion::default(), None).await.unwrap_err());
    assert_eq!(why.keys().collect::<Vec<_>>(), ["NOPE-1"], "a missing id first: {why:?}");
    let why = refused(store.delete(ids(&["M-4", "M-7", "M-5"]), Deletion::default(), None).await.unwrap_err());
    assert_eq!(why.keys().collect::<Vec<_>>(), ["M-7"]);
    assert_eq!(written(&store), before, "nothing written");
    assert!(exists(&store, "M-4"));
    let done = store.delete(Selection::Filter { lens: Lens::parse("in N").unwrap(), max: 500 }, Deletion::default(), None).await.unwrap();
    assert_eq!(done.ids, ["N-1", "N-2"], "a root goes with all its shortcode");
    let refused = store.delete(ids(&["N-2"]), Deletion::default(), None).await.unwrap_err();
    assert!(refused.to_string().contains("N-2 is already deleted"), "{refused}");
    // `may` is asked of every ticket that goes, the subtree's too.
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = asked.clone();
    let no = store
        .delete_by(ids(&["M-7"]), how(Some(Children::Delete), false), Act::default(), move |ids| {
            seen.lock().unwrap().extend(ids.iter().cloned());
            Err("not yours".into())
        })
        .await
        .unwrap_err();
    assert!(no.to_string().contains("not yours"));
    assert_eq!(*asked.lock().unwrap(), ["M-7", "M-8", "M-9"]);
    assert!(exists(&store, "M-8"));
}

#[tokio::test]
async fn undelete_puts_back_what_the_delete_moved() {
    let store = store().await;
    store.delete(ids(&["M-7"]), how(Some(Children::Reparent), false), None).await.unwrap();
    store.delete(ids(&["M-2"]), how(None, true), None).await.unwrap();
    assert_kept(&store, "two deletes");
    let mut rx = store.subscribe();
    let back = store.undelete(&["m-7".into(), "M-2".into()]).await.unwrap();
    assert_eq!(back.ids, ["M-2", "M-7"]);
    assert_eq!(back.tickets[1].body, "the body of an area", "whole, body and all");
    for c in ["M-8", "M-9"] {
        assert_eq!(store.get(c).await.unwrap().parent.as_deref(), Some("M-7"), "{c} back under it");
    }
    assert_eq!(store.get("M-3").await.unwrap().blocked_by, ["M-2"], "the block back");
    assert_eq!(store.get("M-2").await.unwrap().unlocks, ["M-3"]);
    assert_eq!(store.get("M-7").await.unwrap().child_count, 2);
    assert!(store.tombstone("M-2").is_none() && store.tombstone("M-7").is_none());
    let k = kinds(&drained(&mut rx));
    assert_eq!(k[0], "undeleted", "{k:?}");
    assert_kept(&store, "an undelete");
    let refused = store.undelete(&["M-3".into(), "ZZ-1".into()]).await.unwrap_err().to_string();
    assert!(refused.contains("M-3 is not deleted") && refused.contains("ZZ-1"), "{refused}");
}

#[tokio::test]
async fn undelete_is_refused_while_its_parent_is_deleted() {
    let store = store().await;
    store.delete(ids(&["M-7"]), how(Some(Children::Delete), false), None).await.unwrap();
    let why = store.undelete(&["M-8".into()]).await.unwrap_err().to_string();
    assert!(why.contains("M-8 hung under M-7, which is deleted — undelete it too"), "{why}");
    store.undelete(&["M-8".into(), "M-7".into(), "M-9".into()]).await.unwrap();
    assert_kept(&store, "a subtree back");
}

#[tokio::test]
async fn the_purge_keeps_the_tombstone_and_ends_the_undo() {
    let store = store().await;
    let done = store.delete(ids(&["M-6"]), Deletion::default(), None).await.unwrap();
    let at = done.tombstones[0].at_unix;
    assert!(store.purge(at + KEEP_BODY_FOR - 1).await.unwrap().is_empty(), "not yet");
    assert_eq!(store.purge(at + KEEP_BODY_FOR).await.unwrap(), ["M-6"]);
    let t = store.tombstone("M-6").unwrap();
    assert_eq!((t.title.as_str(), t.restorable_until_unix), ("build 6", None));
    let why = store.undelete(&["M-6".into()]).await.unwrap_err().to_string();
    assert!(why.contains("M-6's body was purged"), "{why}");
}

#[tokio::test]
async fn watches_hear_the_deleted_leave_and_the_undeleted_enter() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    let mut builds = store.watcher(Lens::parse("kind build").unwrap());
    let under = store.watcher_in(Lens::parse("under M-7").unwrap(), None);
    let moved = |rx: &mut broadcast::Receiver<Change>| {
        let sent = drained(rx);
        let cells = sent.iter().find_map(|c| match c {
            Change::CellsMoved { moves } => Some(moves.clone()),
            _ => None,
        });
        let unders = sent.iter().find_map(|c| match c {
            Change::UnderMoved { moves } => Some(moves.clone()),
            _ => None,
        });
        (cells.unwrap(), unders.unwrap())
    };
    let m8 = |inn: bool| match inn {
        true => Membership::Entered { id: "M-8".into(), shortcode: ShortcodeId::from("M"), seq: 8 },
        false => Membership::Left { id: "M-8".into(), shortcode: ShortcodeId::from("M"), seq: 8 },
    };
    store.delete(ids(&["M-8"]), Deletion::default(), None).await.unwrap();
    let (cells, unders) = moved(&mut rx);
    assert_eq!(builds.membership(&store, &cells, None), [m8(false)]);
    assert_eq!(under.under_membership(&unders), [m8(false)]);
    store.undelete(&["M-8".into()]).await.unwrap();
    let (cells, unders) = moved(&mut rx);
    assert_eq!(builds.membership(&store, &cells, None), [m8(true)]);
    assert_eq!(under.under_membership(&unders), [m8(true)]);
}

#[tokio::test]
async fn a_deleted_tickets_history_is_kept() {
    let store = store().await;
    store.set_status("M-5", Status::Ready).await.unwrap();
    store.delete(ids(&["M-5"]), Deletion::default(), None).await.unwrap();
    let h = store.history("M-5").unwrap();
    assert_eq!(h.own.iter().map(|t| t.to.as_str()).collect::<Vec<_>>(), ["Pending", "Ready"]);
    // And a history filter does not count it.
    assert!(store.counts_text(&Lens::parse("entered Ready").unwrap()).iter().all(|(_, n)| *n == 0));
}

#[tokio::test]
async fn the_file_holds_what_the_cache_holds_and_verify_agrees() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delete.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    for i in 0..30 {
        store.create(new("M", &format!("more {i}"), Kind::Build, Some("M-7"))).await.unwrap();
    }
    store.set_status("M-4", Status::Ready).await.unwrap();
    store.delete(ids(&["M-7"]), how(Some(Children::Reparent), false), Some("flatten".into())).await.unwrap();
    store.delete(ids(&["M-2"]), how(None, true), None).await.unwrap();
    store.delete(Selection::Filter { lens: Lens::parse("in N").unwrap(), max: 10 }, Deletion::default(), None).await.unwrap();
    store.undelete(&["M-7".into()]).await.unwrap();
    store.delete(ids(&["M-39"]), Deletion::default(), None).await.unwrap();
    assert_kept(&store, "the deletes");
    let rows: Vec<Row> = store.tickets.read().unwrap().order.values().cloned().collect();
    let (groups, extents) = (store.groups(), store.extents_text());
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { tombstones: Some(4), .. } => {}
        other => panic!("{other:?}"),
    }
    let again = Store::open(&path).unwrap();
    let reread: Vec<Row> = again.tickets.read().unwrap().order.values().cloned().collect();
    assert_eq!(reread, rows, "the file holds what the cache held");
    assert_eq!((again.groups(), again.extents_text()), (groups, extents));
    assert_eq!(again.tombstone("M-2").unwrap().unblocked, ["M-3"]);
    assert_eq!(again.history("M-2").unwrap().own.len(), 1, "its birth, kept");
    assert_eq!(again.create(new("M", "after", Kind::Build, None)).await.unwrap().id, "M-40", "past the deleted M-39");
    assert_eq!(again.create(new("N", "n anew", Kind::Scope, None)).await.unwrap().id, "N-3");
}
