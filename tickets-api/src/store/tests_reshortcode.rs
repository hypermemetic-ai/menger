//! ACT-6: `reshortcode` — a test per row of the ticket's table: into a
//! shortcode there is (the next numbers, in the order given; the root, or a
//! parent inside it); `new_scope` minting the shortcode first (a clash
//! refused naming the one there); a scope moved out to root a shortcode of
//! its own; an old id resolving everywhere (`get` with `moved_from`, the
//! resolver, blockers, filters); blockers, children, commits, inputs,
//! comments, placements, history and the queue moving by identity; children
//! staying unless `with_children`; a root moving only as a rename; all or
//! none; `may` asked of the moved and of where they go; `reshortcoded` on
//! watch and `left` / `entered` for a filter; and after each move the derived
//! state what a scan gives, `--verify-derived` consistent, a reopened file
//! the cache and no number given out twice.

use super::tests_update::assert_kept;
use super::*;
use crate::types::ShortcodeMove;

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
/// scope under the root with M-8 and M-9 under it (M-9 waits on M-8); `N`:
/// N-1 its root, N-2 a build; `H`: H-1 a scope hosted under M-7, H-2 a build.
async fn fixture(store: &Store) {
    store.create(new("M", "the root", Kind::Scope, None)).await.unwrap();
    for i in 2..=6 {
        let mut t = new("M", &format!("build {i}"), Kind::Build, None);
        if i == 3 {
            t.blocked_by = vec!["M-2".into()];
        }
        store.create(t).await.unwrap();
    }
    store.create(new("M", "an area", Kind::Scope, Some("M-1"))).await.unwrap();
    store.create(new("M", "eight", Kind::Build, Some("M-7"))).await.unwrap();
    store.create(NewTicket { blocked_by: vec!["M-8".into()], ..new("M", "nine", Kind::Build, Some("M-7")) }).await.unwrap();
    store.create(new("N", "n root", Kind::Scope, None)).await.unwrap();
    store.create(new("N", "n two", Kind::Build, None)).await.unwrap();
    store.create(new("H", "hosted", Kind::Scope, Some("M-7"))).await.unwrap();
    store.create(new("H", "h two", Kind::Build, None)).await.unwrap();
}

async fn store() -> Store {
    let store = Store::new();
    fixture(&store).await;
    store
}

fn ask(ids: &[&str], to: &str) -> Reshortcode {
    Reshortcode { ids: ids.iter().map(|s| s.to_string()).collect(), to: to.into(), ..Reshortcode::default() }
}

fn moves(r: &Reshortcoded) -> Vec<(String, String)> {
    r.moves.iter().map(|m| (m.from.clone(), m.to.clone())).collect()
}

fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
    p.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn exists(store: &Store, id: &str) -> bool {
    store.tickets.read().unwrap().get(id).is_some()
}

fn drained(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    let mut out = Vec::new();
    while let Ok(change) = rx.try_recv() {
        out.push(change);
    }
    out
}

/// Each refused id with its reasons, joined.
fn refused(e: StoreError) -> BTreeMap<String, String> {
    match e {
        StoreError::RefusedMany { refusals } => refusals.into_iter().map(|(id, why)| (id, why.iter().map(|w| w.to_string()).collect::<Vec<_>>().join("; "))).collect(),
        other => panic!("not a refusal of many: {other}"),
    }
}

#[tokio::test]
async fn into_a_shortcode_there_is_each_takes_its_next_number_in_the_order_given_under_its_root() {
    let store = store().await;
    let r = store.reshortcode(ask(&["M-5", "M-4"], "N")).await.unwrap();
    assert_eq!(moves(&r), pairs(&[("M-5", "N-3"), ("M-4", "N-4")]));
    for id in ["N-3", "N-4"] {
        let t = store.get(id).await.unwrap();
        assert_eq!((t.parent.as_deref(), t.epic.as_str(), t.shortcode.as_str()), (Some("N-1"), "N", "N"), "{id}");
    }
    assert_eq!(store.get("N-3").await.unwrap().title, "build 5");
    assert!(!exists(&store, "M-5") && !exists(&store, "M-4"));
    // `parent` must be inside the target.
    let e = store.reshortcode(Reshortcode { parent: Some("M-1".into()), ..ask(&["M-6"], "N") }).await.unwrap_err();
    assert!(e.to_string().contains("parent M-1 is not in N"), "{e}");
    let r = store.reshortcode(Reshortcode { parent: Some("N-2".into()), ..ask(&["M-6"], "N") }).await.unwrap();
    assert_eq!(store.get(&r.moves[0].to).await.unwrap().parent.as_deref(), Some("N-2"));
    // Into its own shortcode: refused.
    let e = store.reshortcode(ask(&["N-2"], "N")).await.unwrap_err();
    assert!(refused(e)["N-2"].contains("is in N already"));
    assert_kept(&store, "the moves into N");
}

#[tokio::test]
async fn new_scope_mints_the_shortcode_first_its_root_under_parent_and_a_clash_is_refused() {
    let store = store().await;
    let r = store
        .reshortcode(Reshortcode { new_scope: Some("the palette".into()), parent: Some("M-7".into()), ..ask(&["M-8", "M-9"], "PAL") })
        .await
        .unwrap();
    assert_eq!(moves(&r), pairs(&[("M-8", "PAL-2"), ("M-9", "PAL-3")]));
    assert_eq!(r.tickets.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["PAL-1", "PAL-2", "PAL-3"]);
    let root = store.get("PAL-1").await.unwrap();
    assert_eq!((root.kind, root.title.as_str(), root.parent.as_deref()), (Kind::Scope, "the palette", Some("M-7")));
    assert_eq!(root.address, "hypermemetic:M/PAL-1", "hosted where its parent is");
    assert_eq!(store.get("PAL-3").await.unwrap().blocked_by, ["PAL-2"], "the block between them, by identity");
    assert_eq!(store.history("PAL-1").unwrap().own.len(), 1, "its birth");
    // H is in M's namespace already: a second H there is refused, naming it.
    let e = store
        .reshortcode(Reshortcode { new_scope: Some("again".into()), parent: Some("M-2".into()), ..ask(&["M-6"], "H") })
        .await
        .unwrap_err();
    assert!(e.to_string().contains("hypermemetic:M/H is already there"), "{e}");
    let e = store.reshortcode(Reshortcode { new_scope: Some("t".into()), ..ask(&["M-6"], "Q") }).await.unwrap_err();
    assert!(e.to_string().contains("under --parent"), "{e}");
    assert!(exists(&store, "M-6"), "nothing written");
    assert_kept(&store, "the new scope");
}

#[tokio::test]
async fn a_scope_moved_into_a_new_code_roots_it_hosted_where_it_hangs_and_its_hosted_shortcodes_follow() {
    let store = store().await;
    let r = store.reshortcode(Reshortcode { with_children: true, ..ask(&["M-7"], "AREA") }).await.unwrap();
    assert_eq!(moves(&r), pairs(&[("M-7", "AREA-1"), ("M-8", "AREA-2"), ("M-9", "AREA-3")]));
    let root = store.get("AREA-1").await.unwrap();
    assert_eq!((root.parent.as_deref(), root.address.as_str()), (Some("M-1"), "hypermemetic:M/AREA-1"));
    assert_eq!(store.get("AREA-2").await.unwrap().parent.as_deref(), Some("AREA-1"));
    // H hung under M-7: it hangs under AREA-1 now, in AREA's namespace.
    let h = store.get("H-1").await.unwrap();
    assert_eq!((h.parent.as_deref(), h.address.as_str()), (Some("AREA-1"), "hypermemetic:M/AREA/H-1"));
    let groups = store.groups();
    assert!(groups.iter().any(|g| g.shortcode.as_str() == "AREA" && g.root.as_deref() == Some("AREA-1")), "{groups:?}");
    // A build can't: it names no shortcode, and roots nothing.
    let e = store.reshortcode(ask(&["M-2"], "NOPE")).await.unwrap_err();
    assert!(e.to_string().contains("pass --new-scope"), "{e}");
    assert_kept(&store, "the scope out");
}

#[tokio::test]
async fn an_old_id_names_the_new_ticket_everywhere_and_get_says_moved_from() {
    let store = store().await;
    store.reshortcode(ask(&["M-2"], "N")).await.unwrap();
    let t = store.get("M-2").await.unwrap();
    assert_eq!((t.id.as_str(), t.moved_from.clone()), ("N-3", vec!["M-2".to_string()]));
    assert_eq!(store.get("m-2").await.unwrap().id, "N-3", "folded as a reader writes it");
    assert_eq!(store.resolve("M-2").unwrap(), "N-3");
    // Its blocked ticket names it anew; a new block by the old id lands on it.
    assert_eq!(store.get("M-3").await.unwrap().blocked_by, ["N-3"]);
    assert_eq!(store.get("N-3").await.unwrap().unlocks, ["M-3"]);
    let born = store.create(NewTicket { blocked_by: vec!["M-2".into()], ..new("M", "waits on the old id", Kind::Build, None) }).await.unwrap();
    assert_eq!(born.blocked_by, ["N-3"]);
    // A filter by the old id, and a set_status by it.
    assert_eq!(store.counts_text(&Lens::parse("under M-2").unwrap()).into_iter().map(|(_, n)| n).sum::<u64>(), 1);
    store.set_status("M-2", Status::Ready).await.unwrap();
    assert_eq!(store.get("N-3").await.unwrap().status, Status::Ready);
    // Moved again: both old ids name it.
    store.reshortcode(ask(&["N-3"], "H")).await.unwrap();
    let t = store.get("M-2").await.unwrap();
    assert_eq!((t.id.as_str(), t.moved_from.clone()), ("H-3", vec!["M-2".to_string(), "N-3".to_string()]));
    // No number is given out twice: N's next is past the one that left.
    assert_eq!(store.create(new("N", "after", Kind::Build, None)).await.unwrap().id, "N-4");
    store.reshortcode(ask(&["M-10"], "N")).await.unwrap();
    assert_eq!(store.create(new("M", "after", Kind::Build, None)).await.unwrap().id, "M-11", "past M-10, which left");
    assert_eq!(store.get("M-10").await.unwrap().title, "waits on the old id", "and M-10 still names the one that left");
    assert_kept(&store, "the old ids");
}

#[tokio::test]
async fn commits_inputs_comments_history_and_the_queue_move_by_identity() {
    let store = store().await;
    store.link_commits("M-4", vec!["77df9a0".into()]).await.unwrap();
    store.set_status("M-4", Status::Ready).await.unwrap();
    let body = r#"<m-input id="notes" kind="text"><m-question data-field="question">Anything?</m-question></m-input>"#;
    store.amend("M-4", None, Some(body.into())).await.unwrap();
    let anchor = crate::comments::Anchor {
        route: "#/ticket/M-4".into(),
        path: "p".into(),
        label: "l".into(),
        excerpt: String::new(),
        ticket: Some("M-4".into()),
        version: None,
    };
    let c = store.add_comment(anchor, "a note".into(), None).await.unwrap();
    store.queue_by("M-4", crate::queue::Urgency::Today, "look", None, Act::default()).await.unwrap();
    store.reshortcode(ask(&["M-4"], "N")).await.unwrap();
    let t = store.get("N-3").await.unwrap();
    assert_eq!(t.commits, ["77df9a0"]);
    assert_eq!(store.inputs(Some("N-3"), None, None).unwrap().iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["notes"]);
    assert_eq!(store.inputs(Some("M-4"), None, None).unwrap().len(), 1, "by the old id too");
    let c = store.comment(c.id).unwrap();
    assert_eq!((c.anchor.ticket.as_deref(), c.anchor.route.as_str()), (Some("N-3"), "#/ticket/N-3"));
    let h = store.history("N-3").unwrap();
    assert_eq!(h.own.iter().map(|t| t.to.as_str()).collect::<Vec<_>>(), ["Pending", "Ready"], "its history, kept");
    assert!(store.queued(true, 0, |_| true).iter().any(|(e, _)| e.ticket == "N-3"), "still queued, as N-3");
    assert_kept(&store, "the records");
}

#[tokio::test]
async fn children_stay_in_their_shortcode_unless_with_children() {
    let store = store().await;
    // Without: M-8 and M-9 stay in M, under M-7's parent; H-1 (its own
    // shortcode) stays hosted under the moved ticket.
    let r = store.reshortcode(ask(&["M-7"], "N")).await.unwrap();
    assert_eq!(moves(&r), pairs(&[("M-7", "N-3")]));
    for id in ["M-8", "M-9"] {
        assert_eq!(store.get(id).await.unwrap().parent.as_deref(), Some("M-1"), "{id}");
    }
    assert_eq!(store.get("H-1").await.unwrap().parent.as_deref(), Some("N-3"));
    assert_eq!(store.get("N-3").await.unwrap().child_count, 1);
    // With: the subtree in its own shortcode comes, the structure kept.
    let store = self::store().await;
    let r = store.reshortcode(Reshortcode { with_children: true, ..ask(&["M-7"], "N") }).await.unwrap();
    assert_eq!(moves(&r), pairs(&[("M-7", "N-3"), ("M-8", "N-4"), ("M-9", "N-5")]));
    assert_eq!(store.get("N-5").await.unwrap().parent.as_deref(), Some("N-3"));
    assert_eq!(store.get("N-5").await.unwrap().blocked_by, ["N-4"]);
    assert_eq!(store.get("N-3").await.unwrap().parent.as_deref(), Some("N-1"));
    assert_kept(&store, "the children");
}

#[tokio::test]
async fn a_root_moves_only_with_its_whole_shortcode_as_a_rename() {
    let store = store().await;
    let e = store.reshortcode(ask(&["H-1"], "N")).await.unwrap_err();
    assert!(refused(e)["H-1"].contains("roots the shortcode H"));
    let e = store.reshortcode(Reshortcode { with_children: true, ..ask(&["H-1", "M-2"], "LOOK") }).await.unwrap_err();
    assert!(refused(e).contains_key("H-1"));
    let r = store.reshortcode(Reshortcode { with_children: true, ..ask(&["H-1"], "LOOK") }).await.unwrap();
    assert_eq!(moves(&r), pairs(&[("H-1", "LOOK-1"), ("H-2", "LOOK-2")]));
    let t = store.get("H-2").await.unwrap();
    assert_eq!((t.id.as_str(), t.address.as_str(), t.parent.as_deref()), ("LOOK-2", "hypermemetic:M/LOOK-2", Some("LOOK-1")));
    assert_eq!(store.get("LOOK-1").await.unwrap().parent.as_deref(), Some("M-7"));
    // A code taken in its namespace is refused: N is a root of the org, M's
    // namespace holds LOOK now.
    let e = store.reshortcode(Reshortcode { with_children: true, ..ask(&["N-1"], "M") }).await.unwrap_err();
    assert!(e.to_string().contains("hypermemetic:M is already there"), "{e}");
    // H stays registered, empty: a ticket filed into it again is numbered
    // past the ones that left, whose ids name LOOK's.
    let h = store.create(new("H", "a new H", Kind::Scope, None)).await.unwrap();
    assert_eq!(h.id, "H-3");
    assert_eq!(store.get("H-1").await.unwrap().id, "LOOK-1");
    assert_kept(&store, "the rename");
}

#[tokio::test]
async fn many_is_all_or_nothing_and_may_is_asked_of_the_moved_and_where_they_go() {
    let store = store().await;
    let e = store.reshortcode(ask(&["M-2", "NOPE-9", "M-3"], "N")).await.unwrap_err();
    assert_eq!(refused(e).keys().collect::<Vec<_>>(), ["NOPE-9"]);
    assert!(exists(&store, "M-2") && exists(&store, "M-3"), "nothing written");
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = asked.clone();
    let e = store
        .reshortcode_by(ask(&["M-2", "M-3"], "N"), Act::default(), move |ids| {
            *seen.lock().unwrap() = ids.to_vec();
            Err("no grant on N".into())
        })
        .await
        .unwrap_err();
    assert!(e.to_string().contains("no grant on N"));
    assert_eq!(*asked.lock().unwrap(), ["M-2", "M-3", "N-1"]);
    assert!(exists(&store, "M-2"), "nothing written");
}

#[tokio::test]
async fn watch_says_reshortcoded_and_a_filter_hears_the_old_leave_and_the_new_enter() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    store.reshortcode(ask(&["M-2", "M-3"], "N")).await.unwrap();
    let changes = drained(&mut rx);
    match &changes[0] {
        Change::Reshortcoded { moves, tickets } => {
            assert_eq!(moves, &[ShortcodeMove { from: "M-2".into(), to: "N-3".into() }, ShortcodeMove { from: "M-3".into(), to: "N-4".into() }]);
            assert_eq!(tickets[1].blocked_by, ["N-3"]);
        }
        other => panic!("{other:?}"),
    }
    let cells = changes.iter().find_map(|c| match c {
        Change::CellsMoved { moves } => Some(moves.clone()),
        _ => None,
    });
    let cells = cells.expect("the cells moved");
    assert!(cells.moves.iter().any(|m| m.shortcode.as_str() == "M" && m.seq == 2 && m.to == cells::GONE));
    assert!(cells.moves.iter().any(|m| m.shortcode.as_str() == "N" && m.seq == 3 && m.from.is_none()));
    // `watch { epic }` of either side hears it.
    let m = ShortcodeId::new("M");
    assert!(changes[0].clone().in_epic(Some(&m)).is_some());
    assert!(changes[0].clone().in_epic(Some(&ShortcodeId::new("H"))).is_none());
}

#[tokio::test]
async fn the_file_holds_what_the_cache_holds_and_verify_agrees() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reshortcode.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    store.link_commits("M-9", vec!["abcdef1".into()]).await.unwrap();
    store.reshortcode(ask(&["M-5", "M-4"], "N")).await.unwrap();
    store.reshortcode(Reshortcode { with_children: true, ..ask(&["M-7"], "AREA") }).await.unwrap();
    store.reshortcode(Reshortcode { with_children: true, ..ask(&["H-1"], "LOOK") }).await.unwrap();
    store.reshortcode(Reshortcode { new_scope: Some("p".into()), parent: Some("M-1".into()), ..ask(&["M-2", "M-3"], "PAL") }).await.unwrap();
    assert_kept(&store, "the moves");
    let rows: Vec<Row> = store.tickets.read().unwrap().order.values().cloned().collect();
    let (groups, extents) = (store.groups(), store.extents_text());
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { .. } => {}
        other => panic!("{other:?}"),
    }
    let again = Store::open(&path).unwrap();
    let reread: Vec<Row> = again.tickets.read().unwrap().order.values().cloned().collect();
    assert_eq!(reread, rows, "the file holds what the cache held");
    assert_eq!((again.groups(), again.extents_text()), (groups, extents));
    for (old, new) in [("M-5", "N-3"), ("M-7", "AREA-1"), ("M-9", "AREA-3"), ("H-2", "LOOK-2"), ("M-3", "PAL-3")] {
        assert_eq!(again.get(old).await.unwrap().id, new, "{old}");
    }
    assert_eq!(again.get("PAL-3").await.unwrap().blocked_by, ["PAL-2"]);
    assert_eq!(again.get("AREA-3").await.unwrap().commits, ["abcdef1"]);
}
