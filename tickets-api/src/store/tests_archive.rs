//! ARCHIVE-2: the archive — every row of the ticket's table, and ARCHIVE-1's
//! decided edges: a subtree archived whole, live work named and never
//! refused; status, blockers and writes unchanged by the mark; every view
//! without the archive unless the filter names it (list, counts, tree,
//! search, find, ready set, member, inputs, queue, waves, health, groups);
//! `get` still answering, with the mark; the roll-ups counting it apart;
//! a dry run writing nothing; restore with its ancestor chain, with
//! children, and what stays named; `archived` / `restored` as filter cells;
//! the queue left and an agent's reply; a `may` that refuses writing
//! nothing; and the derived state a scan, durable, `--verify-derived`
//! consistent. ARCHIVE-6: an archive's tags — kept on every mark it sets,
//! none for an untagged one, `archived tag <name>` answered from the cells,
//! a restore by tag, a bad tag refused, and form 17 gaining them in place.

use super::update_many::Selection;
use super::*;
use crate::queue::{QueueAction, Reply, Urgency};
use crate::types::LiveTicket;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        body: format!("the body of {title}, kiln and glaze"),
    }
}

/// `S`: S-1 the studio's root. `K` hosted under S-1: K-1 its root (a scope),
/// K-2 Complete, K-3 Complete, K-4 In Review, K-5 a doc, K-6 an execution
/// under K-1 with K-7 (Pending) and K-8 (Ready) under it. `G` hosted under
/// S-1: G-1 its root, G-2 blocked by K-3 (Complete), G-3 blocked by K-8.
async fn fixture(store: &Store) {
    store.create(SYS, new("S", "studio", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("K", "kiln", Kind::Scope, Some("S-1"))).await.unwrap();
    for (title, kind) in [("thermocouple", Kind::Build), ("firing log", Kind::Build), ("cone chart", Kind::Build), ("survey", Kind::Doc)] {
        store.create(SYS, new("K", title, kind, None)).await.unwrap();
    }
    store.create(SYS, new("K", "controller", Kind::Execution, None)).await.unwrap();
    store.create(SYS, new("K", "alarm", Kind::Build, Some("K-6"))).await.unwrap();
    store.create(SYS, new("K", "shutoff", Kind::Build, Some("K-6"))).await.unwrap();
    for (id, status) in [("K-2", Status::Complete), ("K-3", Status::Complete), ("K-4", Status::InReview), ("K-8", Status::Ready)] {
        store.set_status(SYS.permit(id), status).await.unwrap();
    }
    store.create(SYS, new("G", "glaze", Kind::Scope, Some("S-1"))).await.unwrap();
    let mut g2 = new("G", "test tiles", Kind::Build, None);
    g2.blocked_by = vec!["K-3".into()];
    store.create(SYS, g2).await.unwrap();
    let mut g3 = new("G", "recipe cards", Kind::Build, None);
    g3.blocked_by = vec!["K-8".into()];
    store.create(SYS, g3).await.unwrap();
    store.set_status(SYS.permit("G-2"), Status::Ready).await.unwrap();
}

async fn store() -> Store {
    let store = Store::new();
    fixture(&store).await;
    store
}

fn ids(ids: &[&str]) -> Selection {
    Selection::Ids(ids.iter().map(|s| s.to_string()).collect())
}

fn texts(v: &[TicketId]) -> Vec<&str> {
    v.iter().map(TicketId::as_str).collect()
}

fn written(store: &Store) -> u64 {
    store.tickets.read().unwrap().written
}

/// The ids a page answers.
fn page_ids(page: &Page) -> Vec<String> {
    page.tickets.iter().map(|t| t.id.to_string()).collect()
}

/// Every id `list { filter }` answers (the store's order).
fn listed(store: &Store, filter: &str) -> Vec<String> {
    let q = Filtered { lens: Lens::parse(filter).unwrap(), epic: None, status: None, parent: None, keep: None };
    page_ids(&store.filtered_page(SYS, &q, None, None, None).unwrap())
}

/// The derived state is what a scan of the rows builds — cells, tree,
/// roll-ups, ready set, everything `--verify-derived` compares.
fn assert_derived(store: &Store, after: &str) {
    let cache = store.tickets.read().unwrap();
    let scanned = Cache::new(cache.order.values().map(|t| (t.id.to_string(), t.clone())).collect(), cache.inputs.clone());
    assert_eq!(stored::compare(&cache.form(), &scanned.form()), Vec::<String>::new(), "after {after}");
}

#[tokio::test]
async fn a_subtree_is_archived_whole_its_live_work_named_never_refused() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    let before = written(&store);
    let a = store.archive_by(ids(&["k-1"]), Act { actor: "owner".into(), message: Some("first season fired".into()), ..Act::default() }, false, |ids| Ok(SYS.permits(ids))).await.unwrap();
    assert_eq!(written(&store), before + 1, "one write");
    assert_eq!(texts(&a.ids), ["K-1", "K-2", "K-3", "K-4", "K-5", "K-6", "K-7", "K-8"], "the root and its whole subtree, in the store's order");
    let live: Vec<(&str, Status)> = a.live.iter().map(|LiveTicket { id, status }| (id.as_str(), *status)).collect();
    assert_eq!(live, [("K-1", Status::Draft), ("K-4", Status::InReview), ("K-6", Status::Draft), ("K-7", Status::Draft), ("K-8", Status::Ready)], "each not done, named");
    assert!(!a.dry_run && a.mark.by == "owner" && a.mark.message.as_deref() == Some("first season fired") && a.mark.write > 0);
    // A mark apart from status: everything else as it was.
    let k4 = store.get(SYS, "K-4").await.unwrap();
    assert_eq!((k4.status, k4.kind, k4.parent.as_deref()), (Status::InReview, Kind::Build, Some("K-1")));
    assert_eq!(k4.archived.as_ref(), Some(&a.mark), "get answers the ticket, with its mark");
    // One transition each, the first the cause of the rest.
    let h = store.history(SYS, "K-1").unwrap();
    let own = h.own.iter().find(|t| t.field == crate::types::TransitionField::Archive).unwrap();
    assert_eq!((own.from.as_deref(), own.to.as_str(), own.actor.as_str(), own.id), (Some("live"), "archived", "owner", a.mark.write));
    assert_eq!(h.caused.iter().filter(|t| t.field == crate::types::TransitionField::Archive).count(), 7, "the write's other tickets, caused by it");
    // On watch: transitions, then `archived`, then the cells moved.
    let sent: Vec<Change> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let at = sent.iter().position(|c| matches!(c, Change::Archived { .. })).expect("archived on watch");
    assert!(sent[at + 1..].iter().all(|c| matches!(c, Change::CellsMoved { .. } | Change::UnderMoved { .. })), "the filtered moves last");
    assert!(sent[at + 1..].iter().any(|c| matches!(c, Change::CellsMoved { .. })));
    // Already archived: a no-op, no write.
    let again = store.archive(SYS, ids(&["K-4", "K-1"]), false).await.unwrap();
    assert!(again.ids.is_empty() && again.live.is_empty());
    assert_eq!(written(&store), before + 1);
    assert_derived(&store, "the archive");
}

#[tokio::test]
async fn a_dry_run_answers_and_writes_nothing() {
    let store = store().await;
    let before = written(&store);
    let mut rx = store.subscribe();
    let dry = store.archive(SYS, ids(&["K-6"]), true).await.unwrap();
    assert_eq!((texts(&dry.ids), dry.dry_run, dry.mark.write), (vec!["K-6", "K-7", "K-8"], true, 0));
    assert_eq!(dry.live.len(), 3);
    assert_eq!(written(&store), before, "nothing written");
    assert!(rx.try_recv().is_err(), "nothing broadcast");
    assert!(store.get(SYS, "K-7").await.unwrap().archived.is_none());
    let wet = store.archive(SYS, ids(&["K-6"]), false).await.unwrap();
    assert_eq!(wet.ids, dry.ids, "the write does what the dry run said");
}

#[tokio::test]
async fn every_view_leaves_the_archive_out_unless_the_filter_names_it() {
    let store = store().await;
    let all_before = store.page(SYS, PageQuery::default()).unwrap().tickets.len();
    store.archive(SYS, ids(&["K-1"]), false).await.unwrap();
    // list: by epic, status, parent — and under a filter that says nothing.
    let page = store.page(SYS, PageQuery::default()).unwrap();
    assert_eq!(page.tickets.len(), all_before - 8);
    assert!(page.tickets.iter().all(|t| t.archived.is_none()));
    assert!(page_ids(&store.page(SYS, PageQuery { epic: Some("K"), ..PageQuery::default() }).unwrap()).is_empty());
    assert!(page_ids(&store.page(SYS, PageQuery { status: Some(Status::Complete), ..PageQuery::default() }).unwrap()).is_empty());
    assert!(listed(&store, "status Draft").iter().all(|id| !id.starts_with("K-")), "a filter that says nothing about it");
    // …and the archive, when asked for.
    assert_eq!(listed(&store, "status Draft and archived"), ["K-1", "K-6", "K-7"]);
    assert_eq!(listed(&store, "archived").len(), 8);
    assert_eq!(listed(&store, "archived or in G"), ["G-1", "G-2", "G-3", "K-1", "K-2", "K-3", "K-4", "K-5", "K-6", "K-7", "K-8"]);
    assert!(listed(&store, "not archived").iter().all(|id| !id.starts_with("K-")));
    // The full-ticket walk too.
    let walked = store.list(SYS, Some("K"), None, None).await.unwrap();
    assert!(walked.is_empty());
    // counts, tree, search, find, member.
    let counted = |f: &str| store.counts(SYS, &Lens::parse(f).unwrap()).into_iter().map(|(_, n)| n).sum::<u64>();
    assert_eq!((counted(""), counted("archived"), counted("in K and archived")), (all_before as u64 - 8, 8, 8));
    let tree = |f: &str| {
        let q = TreeQuery { parent: Some("S-1".into()), epic: None, lens: Lens::parse(f).unwrap(), matches_only: false, keep: None };
        store.tree_page(SYS, &q, None, None).unwrap().nodes.into_iter().map(|n| (n.summary.id.to_string(), n.below)).collect::<Vec<_>>()
    };
    assert_eq!(tree("not category done"), [("G-1".to_string(), 2)], "the kept tree's tallies, no walk");
    assert_eq!(tree("archived"), [("K-1".to_string(), 7)]);
    let hits = |lens: Option<Lens>| async {
        store.search(SYS, "kiln glaze", lens, None, None).await.unwrap().hits.into_iter().map(|h| h.summary.id.to_string()).collect::<Vec<_>>()
    };
    assert!(hits(None).await.iter().all(|id| !id.starts_with("K-")), "search without a filter");
    assert_eq!(hits(Some(Lens::parse("archived").unwrap())).await.len(), 8);
    assert!(page_ids(&store.find_page(SYS, "K-", None, None).unwrap()).is_empty(), "find");
    assert_eq!(store.member(SYS, &["K-2".into(), "G-2".into()], &Lens::live()), [false, true]);
    assert_eq!(store.member(SYS, &["K-2".into()], &Lens::parse("archived").unwrap()), [true]);
    assert_eq!(store.member(SYS, &["K-2".into()], &Lens::all()), [true], "an access check reaches it");
    // health: the archive counted apart.
    assert_eq!((store.count(SYS), store.archived_count(SYS)), (all_before - 8, 8));
    // waves place none of it.
    assert!(store.waves(SYS, &["K".into()], &Kind::ALL).placed.is_empty());
    // get, by any name, still answers.
    assert!(store.get(SYS, "k-3").await.unwrap().archived.is_some());
}

#[tokio::test]
async fn the_roll_ups_count_the_archive_apart_and_an_archived_root_leaves_the_groups() {
    let store = store().await;
    let group = |s: &Store, sc: &str| s.groups(SYS).into_iter().find(|g| g.shortcode.as_str() == sc);
    store.archive(SYS, ids(&["K-6"]), false).await.unwrap();
    let k = group(&store, "K").unwrap();
    assert_eq!((k.total, k.done, k.docs, k.archived), (4, 2, 1, 3), "done/total reads the live work");
    store.archive(SYS, ids(&["K-1"]), false).await.unwrap();
    assert!(group(&store, "K").is_none(), "an archived root's shortcode leaves the groups");
    let s = group(&store, "S").unwrap();
    assert_eq!((s.total, s.archived), (4, 8), "the host's row: its live work, the archive apart");
    store.restore(SYS, ids(&["K-1"]), true, false).await.unwrap();
    let k = group(&store, "K").unwrap();
    assert_eq!((k.total, k.archived), (4, 3), "K-6's subtree was archived by another write: it stays");
    assert_derived(&store, "archive and restore");
}

#[tokio::test]
async fn blockers_and_status_are_unchanged_and_writes_to_the_archive_are_allowed() {
    let store = store().await;
    let ready_ids = |s: &Store| page_ids(&s.ready_page(SYS, None, None, None).unwrap());
    assert!(ready_ids(&store).contains(&"G-2".to_string()));
    store.archive(SYS, ids(&["K-1"]), false).await.unwrap();
    assert!(ready_ids(&store).contains(&"G-2".to_string()), "an archived Complete blocker still satisfies");
    assert!(!ready_ids(&store).contains(&"K-8".to_string()), "an archived Ready ticket is in no ready set");
    let g3 = store.get(SYS, "G-3").await.unwrap();
    assert_eq!(g3.blocked_by, ["K-8"], "an archived open blocker still blocks, named");
    let blockers = store.blockers(SYS, "G-3").await.unwrap();
    assert!(matches!(&blockers[..], [BlockerRef::Resolved(t)] if t.archived.is_some()), "shown as archived, never hidden");
    // A write to an archived ticket: allowed, and it stays archived.
    assert_eq!(listed(&store, "in G and blocked"), ["G-3"]);
    store.set_status(SYS.permit("K-8"), Status::Complete).await.unwrap();
    store.label(SYS.permit("K-8"), vec!["late".into()], vec![]).await.unwrap();
    let k8 = store.get(SYS, "K-8").await.unwrap();
    assert_eq!((k8.status, k8.archived.is_some()), (Status::Complete, true));
    assert!(listed(&store, "in G and blocked").is_empty(), "its completion unblocks what waits on it");
    let k = store.groups(SYS).into_iter().find(|g| g.shortcode.as_str() == "S").unwrap();
    assert_eq!(k.archived, 8, "an archived ticket's completion moves no `done`");
    assert_derived(&store, "writes to the archive");
}

#[tokio::test]
async fn restore_brings_back_the_ancestor_chain_never_its_other_children() {
    let store = store().await;
    store.archive(SYS, ids(&["K-1"]), false).await.unwrap();
    let dry = store.restore(SYS, ids(&["K-7"]), false, true).await.unwrap();
    assert_eq!((texts(&dry.ids), texts(&dry.ancestors)), (vec!["K-1", "K-6", "K-7"], vec!["K-1", "K-6"]), "the dry run names the chain");
    assert!(store.get(SYS, "K-6").await.unwrap().archived.is_some(), "nothing written");
    let r = store.restore(SYS, ids(&["K-7"]), false, false).await.unwrap();
    assert_eq!(texts(&r.ids), ["K-1", "K-6", "K-7"]);
    for (id, archived) in [("K-1", false), ("K-6", false), ("K-7", false), ("K-8", true), ("K-2", true)] {
        assert_eq!(store.get(SYS, id).await.unwrap().archived.is_some(), archived, "{id}");
    }
    // Restoring a parent alone names what stays archived below it.
    store.archive(SYS, ids(&["K-6"]), false).await.unwrap();
    let r = store.restore(SYS, ids(&["K-6"]), false, false).await.unwrap();
    assert_eq!((texts(&r.ids), texts(&r.still_archived)), (vec!["K-6"], vec!["K-7", "K-8"]));
    // With children: the subtree archived in the same write as it.
    store.restore(SYS, ids(&["K-7"]), false, false).await.unwrap();
    store.archive(SYS, ids(&["K-6"]), false).await.unwrap();
    let r = store.restore(SYS, ids(&["K-6"]), true, false).await.unwrap();
    assert_eq!(texts(&r.ids), ["K-6", "K-7"], "K-8 was archived before, by another write");
    assert_eq!(texts(&r.still_archived), ["K-8"]);
    assert_derived(&store, "the restores");
}

#[tokio::test]
async fn a_create_or_move_under_an_archived_parent_finds_its_chain() {
    let store = store().await;
    store.archive(SYS, ids(&["K-1"]), false).await.unwrap();
    assert_eq!(store.archived_chain("K-6"), ["K-1", "K-6"], "the parent and every archived ancestor");
    assert!(store.archived_chain("G-1").is_empty());
    store.create(SYS, new("K", "new alarm", Kind::Build, Some("K-6"))).await.unwrap();
    let r = store.restore_chain_by(store.archived_chain("K-6"), Act::default(), SYS.permits(["K-1", "K-6"])).await.unwrap().unwrap();
    assert_eq!((texts(&r.ids), texts(&r.ancestors), r.still_archived.len()), (vec!["K-1", "K-6"], vec!["K-1", "K-6"], 0));
    assert!(store.get(SYS, "K-7").await.unwrap().archived.is_some(), "the chain only");
    assert!(store.restore_chain_by(vec!["K-1".into()], Act::default(), SYS.permits(["K-1"])).await.unwrap().is_none(), "nothing left to restore");
}

#[tokio::test]
async fn by_filter_capped_and_a_may_that_refuses_writes_nothing() {
    let store = store().await;
    let f = |text: &str, max: usize| Selection::Filter { lens: Lens::parse(text).unwrap(), max };
    let refused = store.archive(SYS, f("in K", 3), false).await.unwrap_err();
    assert!(refused.to_string().contains("admits 8 tickets, over the 3"), "{refused}");
    let a = store.archive(SYS, f("category done and in K", 500), false).await.unwrap();
    assert_eq!(texts(&a.ids), ["K-2", "K-3"]);
    let before = written(&store);
    let e = store.archive_by(ids(&["K-1"]), Act::default(), false, |ids| Err(format!("{} outside your reach", ids.len()))).await.unwrap_err();
    assert!(e.to_string().contains("6 outside your reach"), "{e}: its subtree's tickets asked, the archived two not");
    assert_eq!(written(&store), before);
    // A restore by filter: of the archive only.
    let r = store.restore(SYS, f("archived and in K", 500), false, false).await.unwrap();
    assert_eq!(texts(&r.ids), ["K-2", "K-3"]);
}

#[tokio::test]
async fn archived_and_restored_are_filter_cells_read_from_the_log() {
    let store = store().await;
    let by = |actor: &str| Act { actor: actor.into(), ..Act::default() };
    store.archive_by(ids(&["K-6"]), by("owner"), false, |ids| Ok(SYS.permits(ids))).await.unwrap();
    store.archive_by(ids(&["K-2"]), by("ada"), false, |ids| Ok(SYS.permits(ids))).await.unwrap();
    assert_eq!(listed(&store, "archived within 7d by owner"), ["K-6", "K-7", "K-8"]);
    assert_eq!(listed(&store, "archived by ada"), ["K-2"]);
    assert!(listed(&store, "archived before 2000-01-01").is_empty());
    store.restore_by(ids(&["K-7"]), false, by("ada"), false, |ids| Ok(SYS.permits(ids))).await.unwrap();
    assert_eq!(listed(&store, "archived within 7d by owner"), ["K-8"], "an archive no longer standing is not one");
    assert_eq!(listed(&store, "restored today by ada"), ["K-6", "K-7"], "restored: live again");
    assert!(listed(&store, "restored by owner").is_empty());
    let parsed = crate::filter::Filter::parse("restored within 7d").unwrap();
    assert!(!parsed.names_archived(), "restored does not lift the default");
    assert!(crate::filter::Filter::parse("archived within 7d by me").unwrap().names_archived());
}

#[tokio::test]
async fn an_archived_ticket_asks_nothing() {
    let store = store().await;
    store.queue_by(SYS.permit("K-7"), Urgency::Today, "is the alarm loud enough?", None, Act::default()).await.unwrap();
    let since = chrono::Utc::now().timestamp() - 1;
    let a = store.archive_by(ids(&["K-1"]), Act { actor: "owner".into(), ..Act::default() }, false, |ids| Ok(SYS.permits(ids))).await.unwrap();
    assert_eq!(texts(&a.unqueued), ["K-7"]);
    assert!(store.queued(SYS, true, chrono::Utc::now().timestamp()).is_empty(), "in no tray");
    let records = store.queue_history(SYS, "K-7");
    assert_eq!(records.last().map(|r| r.action), Some(QueueAction::Archived), "recorded as by the archive");
    let reply = store.reply_since(SYS, &replies::Awaiting { ticket: "K-7".into(), since, input: None, me: None });
    assert!(matches!(reply, Some(Reply::Archived { ref by, .. }) if by == "owner"), "{reply:?}");
}

#[tokio::test]
async fn the_archive_is_durable_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    store.archive(SYS, ids(&["K-6"]), false).await.unwrap();
    store.archive(SYS, ids(&["G-2"]), false).await.unwrap();
    store.restore(SYS, ids(&["K-7"]), false, false).await.unwrap();
    let mark = store.get(SYS, "K-8").await.unwrap().archived.unwrap();
    super::tests_cells::assert_cells_are_a_scan(&store, Some(&path), "the archive");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(SYS, "K-8").await.unwrap().archived, Some(mark), "the mark, as kept");
    assert_eq!(listed(&store, "archived"), ["G-2", "K-8"]);
    assert_eq!(listed(&store, "restored"), ["K-6", "K-7"]);
    assert_derived(&store, "the reopen");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }), "{:?}", Store::verify_derived(&path).unwrap());
}

#[tokio::test]
async fn a_random_walk_with_archives_holds_its_kept_state_to_a_scan() {
    use super::tests_ready::{random_write, Rng, Tally};
    let store = Store::new();
    let (mut rng, mut tally) = (Rng(0xA2C4_1E2D_0000_0042), Tally::archiving());
    for i in 0..360 {
        let what = random_write(&store, &mut rng, &["A", "B", "C"], &mut tally).await;
        assert_derived(&store, &what);
        if i % 12 == 0 {
            super::tests_cells::assert_cells_are_a_scan(&store, None, &what);
            let cache = store.tickets.read().unwrap();
            let archived: Vec<String> = cache.order.values().filter(|t| t.archived.is_some()).map(|t| t.id.to_string()).collect();
            drop(cache);
            let live: Vec<String> = store.page(SYS, PageQuery { limit: Some(10_000), ..PageQuery::default() }).unwrap().tickets.into_iter().map(|t| t.id.to_string()).collect();
            assert!(live.iter().all(|id| !archived.contains(id)), "a view never holds the archive, after {what}");
            assert_eq!(listed(&store, "archived"), archived, "the archive, asked for, after {what}");
            // ARCHIVE-6: the walk tags some archives; each tag filter, from
            // the cells, is what the rows' marks say.
            let plain = super::tests_filtered::Plain::of(&store);
            for text in ["archived tag season-1", "archived tag in (kiln, season-1)", "not archived tag kiln", "archived and not archived tag season-1", "(archived tag kiln or status Ready)"] {
                let filter = crate::filter::Filter::parse(text).unwrap();
                let judge = |t: &Row, k: super::cells::Key| filter.matches(&plain.facts(t, k));
                super::tests_filtered::assert_lens_is_a_scan(&store, &plain, &Lens::parse(text).unwrap(), &judge, &mut rng, &format!("{text} after {what}"));
            }
            assert_eq!(store.count(SYS) + archived.len(), store.tickets.read().unwrap().len(), "after {what}");
        }
    }
    assert!(tally.archived >= 20, "the walk archived and restored: {}", tally.archived);
    let tagged = store.tickets.read().unwrap().order.values().filter(|t| t.archived.as_ref().is_some_and(|m| !m.tags.is_empty())).count();
    assert!(tagged > 0, "the walk left tagged archives standing");
}

/// The marks' tags, by id, of `ids`.
async fn tags_of(store: &Store, ids: &[&str]) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for id in ids {
        let t = store.get(SYS, id).await.unwrap();
        out.push((id.to_string(), t.archived.map(|m| m.tags.into_iter().map(String::from).collect()).unwrap_or_default()));
    }
    out
}

#[tokio::test]
async fn a_tagged_archive_keeps_its_tags_on_every_mark_and_the_filter_finds_them() {
    let store = store().await;
    let mut rx = store.subscribe_filtered();
    let a = store.archive_tagged(SYS, ids(&["K-1"]), &["Season-1", "kiln", "season-1"], false).await.unwrap();
    let want: Vec<String> = vec!["season-1".into(), "kiln".into()];
    assert_eq!(a.mark.tags, want, "lower-cased, in the order given, each once");
    assert_eq!(texts(&a.ids), ["K-1", "K-2", "K-3", "K-4", "K-5", "K-6", "K-7", "K-8"]);
    for (id, tags) in tags_of(&store, &["K-1", "K-4", "K-8"]).await {
        assert_eq!(tags, want, "{id}: `get` carries the mark's tags");
    }
    let sent: Vec<Change> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(sent.iter().any(|c| matches!(c, Change::Archived { mark, .. } if mark.tags == want)), "watch's `archived` carries them");
    // On the wire: the mark's `tags`.
    let wire = serde_json::to_value(&a.mark).unwrap();
    assert_eq!(wire["tags"], serde_json::json!(["season-1", "kiln"]));
    // The filter: from the cells, the archive named.
    assert_eq!(listed(&store, "archived tag season-1"), ["K-1", "K-2", "K-3", "K-4", "K-5", "K-6", "K-7", "K-8"]);
    assert_eq!(listed(&store, "archived tag kiln and status Complete"), ["K-2", "K-3"]);
    assert_eq!(listed(&store, "archived tag in (other, kiln) and kind doc"), ["K-5"]);
    assert!(listed(&store, "archived tag other").is_empty());
    assert!(listed(&store, "status Complete").is_empty(), "a filter that does not name the archive still hides it");
    let counted = |f: &str| store.counts(SYS, &Lens::parse(f).unwrap()).into_iter().map(|(_, n)| n).sum::<u64>();
    assert_eq!(counted("archived tag season-1"), 8);
    // Already archived: a no-op — its tags are the write's that archived it.
    let again = store.archive_tagged(SYS, ids(&["K-6"]), &["other"], false).await.unwrap();
    assert!(again.ids.is_empty());
    assert_eq!(tags_of(&store, &["K-6"]).await[0].1, want);
    assert_derived(&store, "a tagged archive");
}

#[tokio::test]
async fn an_archive_with_no_tag_is_as_it_was() {
    let store = store().await;
    store.archive_tagged(SYS, ids(&["K-6"]), &["season-1"], false).await.unwrap();
    let a = store.archive(SYS, ids(&["K-3"]), false).await.unwrap();
    assert!(a.mark.tags.is_empty());
    assert_eq!(texts(&a.ids), ["K-3"]);
    let wire = serde_json::to_value(&a.mark).unwrap();
    assert!(wire.get("tags").is_none(), "no tag: omitted on the wire — {wire}");
    assert!(store.get(SYS, "K-3").await.unwrap().archived.unwrap().tags.is_empty());
    assert_eq!(listed(&store, "archived tag season-1"), ["K-6", "K-7", "K-8"], "the untagged archive is not answered");
    assert_eq!(listed(&store, "archived"), ["K-3", "K-6", "K-7", "K-8"]);
    assert_eq!(listed(&store, "archived and not archived tag season-1"), ["K-3"]);
    // A mark stored before tags were reads with none.
    let old: crate::types::Archived = serde_json::from_str(r#"{"at_unix":1,"by":"owner","write":7}"#).unwrap();
    assert!(old.tags.is_empty());
    assert_derived(&store, "tagged and untagged");
}

#[tokio::test]
async fn a_restore_by_tag_brings_back_what_was_archived_with_it() {
    let store = store().await;
    store.archive_tagged(SYS, ids(&["K-6"]), &["season-1"], false).await.unwrap();
    store.archive(SYS, ids(&["K-2"]), false).await.unwrap();
    store.archive_tagged(SYS, ids(&["G-2"]), &["season-2"], false).await.unwrap();
    // As the activation sends it: the filter, and `archived`.
    let sel = Selection::Filter { lens: Lens::parse("archived tag season-1 and archived").unwrap(), max: 500 };
    let r = store.restore(SYS, sel, false, false).await.unwrap();
    assert_eq!(texts(&r.ids), ["K-6", "K-7", "K-8"]);
    assert!(listed(&store, "archived tag season-1").is_empty());
    assert_eq!(listed(&store, "archived"), ["G-2", "K-2"], "one archived without the tag, and one with another, stay");
    assert!(store.get(SYS, "K-7").await.unwrap().archived.is_none());
    assert_derived(&store, "a restore by tag");
}

#[tokio::test]
async fn a_bad_tag_is_refused_and_nothing_written() {
    let store = store().await;
    let before = written(&store);
    for bad in [&["ok", "a_b"][..], &["two words"], &[""], &[&*"a".repeat(33)]] {
        let e = store.archive_tagged(SYS, ids(&["K-6"]), bad, false).await.unwrap_err();
        assert!(e.to_string().contains("a tag is a label"), "{bad:?}: {e}");
    }
    assert_eq!(written(&store), before);
    assert!(store.get(SYS, "K-6").await.unwrap().archived.is_none());
    assert!(crate::filter::Filter::parse("archived tag a_b").is_err(), "nor named in a filter");
}

#[tokio::test]
async fn a_form_17_file_gains_archive_tags_in_place_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form17.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    store.archive(SYS, ids(&["K-6"]), false).await.unwrap();
    let mark = store.get(SYS, "K-7").await.unwrap().archived.unwrap();
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE cells DROP COLUMN archive_tags; UPDATE derived_state SET form = 17;").unwrap();
    // A form-17 mark: its JSON has no `tags`.
    let stored: String = conn.query_row("SELECT archived FROM tickets WHERE id = 'K-7'", [], |r| r.get(0)).unwrap();
    assert!(!stored.contains("tags"), "{stored}");
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("form 17; this build reads 18")));
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(SYS, "K-7").await.unwrap().archived, Some(mark), "the old mark reads, with no tag");
    assert_eq!(listed(&store, "archived"), ["K-6", "K-7", "K-8"]);
    store.archive_tagged(SYS, ids(&["G-2"]), &["season-1"], false).await.unwrap();
    assert_eq!(listed(&store, "archived tag season-1"), ["G-2"]);
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(conn.query_row("SELECT form FROM derived_state", [], |r| r.get::<_, i64>(0)).unwrap(), FORM);
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }), "{:?}", Store::verify_derived(&path).unwrap());
    let store = Store::open(&path).unwrap();
    assert_eq!(listed(&store, "archived tag season-1"), ["G-2"], "the reopen reads the tags from the cells");
}

#[tokio::test]
async fn a_form_11_file_gains_the_archives_columns_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form11.db");
    let store = Store::open(&path).unwrap();
    fixture(&store).await;
    let groups = store.groups(SYS);
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "ALTER TABLE cells DROP COLUMN archived; ALTER TABLE rollup_groups DROP COLUMN base_archived;
         ALTER TABLE rollup_groups DROP COLUMN own_archived; ALTER TABLE rollup_groups DROP COLUMN archived;
         ALTER TABLE rollup_cross DROP COLUMN archived; ALTER TABLE tickets DROP COLUMN archived;
         UPDATE derived_state SET form = 11;",
    )
    .unwrap();
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains(&format!("form 11; this build reads {FORM}"))));
    let store = Store::open(&path).unwrap();
    assert_eq!(store.groups(SYS), groups, "answers as before, nothing built again");
    store.archive(SYS, ids(&["K-6"]), false).await.unwrap();
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(conn.query_row("SELECT form FROM derived_state", [], |r| r.get::<_, i64>(0)).unwrap(), FORM);
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}
