//! FILTER-13: the kept tree. Every tally and every holder held to an oracle
//! written out here from the rows alone — each ticket's cell counted in each
//! of its strict ancestors by the lineage's rule, each long list's children
//! whose tally holds a cell — after every one of hundreds of random writes
//! of every kind (re-parents included), in the cache and in the file; on
//! stores built by create, by a nested seed, from the fixtures, and from
//! hand-edited shapes (parent cycles, a parent that names no ticket) through
//! the writes that repair them. Then a reopen, which reads what was kept.
//!
//! Under test a list is a bitmap past 4 children ([`HOLD_OVER`]), so the
//! walks cross between the two representations both ways, again and again.

use super::cells::{self, Key};
use crate::address::ShortcodeId;
use std::sync::Arc;
use super::children::HOLD_OVER;
use super::stored::{self, State};
use super::tests_cells::scanned;
use super::tests_ready::{new_ticket, random_write, rows, Rng, Tally};
use super::tree;
use super::*;

/// A ticket's strict ancestors by the lineage's rule, from the rows alone:
/// its parent, that one's, … — each once; the chain ends at an id that
/// names no ticket (the last), at a ticket with no parent, or where it
/// would meet one already met or the ticket itself.
pub(super) fn ancestors_of(by_id: &HashMap<&str, &Row>, t: &Row) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = t.parent.clone();
    while let Some(p) = cur {
        if p == t.id || out.contains(&p.to_string()) {
            break;
        }
        out.push(p.to_string());
        cur = by_id.get(p.as_str()).and_then(|r| r.parent.clone());
    }
    out
}

/// Every tally entry: (parent, shortcode, cell) → count.
pub(super) type Tallies = BTreeMap<(String, String, Key), u32>;
/// Every holder bitmap: (parent, the list's shortcode, the cell's
/// shortcode, cell) → its members.
pub(super) type Holders = BTreeMap<(String, String, String, Key), Vec<u32>>;

/// The oracle: every tally and every holder, from the rows and their cells.
pub(super) fn oracle(rows: &[Row], asking: &std::collections::BTreeSet<String>) -> (Tallies, Holders) {
    let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
    let cells = scanned(rows, asking);
    let mut tallies = Tallies::new();
    for t in rows {
        let k = cells[&OrderKey::of(t)];
        for a in ancestors_of(&by_id, t) {
            *tallies.entry((a, t.epic.to_string(), k)).or_insert(0) += 1;
        }
    }
    let mut by_parent: BTreeMap<&str, Vec<(&str, Key)>> = BTreeMap::new();
    for ((p, e, k), _) in &tallies {
        by_parent.entry(p).or_default().push((e, *k));
    }
    let mut lists: BTreeMap<(String, String), Vec<&Row>> = BTreeMap::new();
    for t in rows {
        if let Some(p) = &t.parent {
            lists.entry((p.to_string(), t.epic.to_string())).or_default().push(t);
        }
    }
    let mut holders = Holders::new();
    for ((p, e), kids) in &lists {
        if kids.len() <= HOLD_OVER {
            continue;
        }
        for c in kids {
            for (ge, k) in by_parent.get(c.id.as_str()).into_iter().flatten() {
                holders.entry((p.clone(), e.clone(), ge.to_string(), *k)).or_default().push(c.seq);
            }
        }
    }
    for v in holders.values_mut() {
        v.sort_unstable();
    }
    (tallies, holders)
}

/// What the cache keeps, in the oracle's terms — and each list a bitmap
/// exactly when it is longer than [`HOLD_OVER`].
pub(super) fn kept(store: &Store) -> (Tallies, Holders) {
    let cache = store.tickets.read().unwrap();
    let (keys, names) = (cache.cells.keys(), cache.children.names());
    let (mut tallies, mut holders) = (Tallies::new(), Holders::new());
    for (p, kids) in cache.children.every() {
        assert!(kids.tally.windows(2).all(|w| w[0].0 < w[1].0), "{p}'s tally is sorted");
        for (g, n) in &kids.tally {
            assert!(*n > 0, "{p}: no 0 is kept");
            tallies.insert((p.to_string(), names.name(tree::g_epic(*g)).to_string(), keys[tree::g_cell(*g) as usize]), *n);
        }
        for (e, seqs) in kids.lists() {
            assert_eq!(seqs.many().is_some(), seqs.len() as usize > HOLD_OVER, "{p}'s list in {e}: {} children", seqs.len());
            let Some(many) = seqs.many() else { continue };
            assert!(many.holders.windows(2).all(|w| w[0].0 < w[1].0), "{p}'s holders are sorted");
            for (g, bm) in &many.holders {
                assert!(!bm.is_empty(), "{p}: no empty holder is kept");
                holders.insert(
                    (p.to_string(), e.to_string(), names.name(tree::g_epic(*g)).to_string(), keys[tree::g_cell(*g) as usize]),
                    bm.iter().collect(),
                );
            }
        }
    }
    (tallies, holders)
}

/// The kept tree the file holds, in the oracle's terms, and as stored.
fn filed(path: &Path) -> (State, Tallies, Holders, tree::Stored) {
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let tx = conn.transaction().unwrap();
    let state = stored::state(&tx).unwrap();
    let form = stored::read(&tx).unwrap();
    let keys = &form.cells.keys;
    let tallies = form.tree.tallies.iter().map(|((p, e, c), n)| ((p.clone(), e.as_str().to_string(), keys[c]), *n)).collect();
    let mut holders = Holders::new();
    for ((p, l, e, c, hi), (card, raw)) in &form.tree.holders {
        let v = holders.entry((p.clone(), l.as_str().to_string(), e.as_str().to_string(), keys[c])).or_default();
        v.extend(cells::decode(*card, raw).unwrap().into_iter().map(|low| (hi << 16) | u32::from(low)));
    }
    (state, tallies, holders, form.tree)
}

/// The kept tree is the oracle's — in the cache, and in the file.
#[track_caller]
pub(super) fn assert_tree_is_a_scan(store: &Store, path: Option<&Path>, after: &str) {
    let (want_tallies, want_holders) = oracle(&rows(store), &super::tests_cells::asking(store));
    let (tallies, holders) = kept(store);
    assert_eq!(tallies, want_tallies, "the cache's tallies, after {after}");
    assert_eq!(holders, want_holders, "the cache's holders, after {after}");
    if let Some(path) = path {
        let (state, file_tallies, file_holders, file) = filed(path);
        assert_eq!(state, State::Current, "after {after}");
        assert_eq!(file_tallies, want_tallies, "the file's tallies, after {after}");
        assert_eq!(file_holders, want_holders, "the file's holders, after {after}");
        let cache = store.tickets.read().unwrap();
        assert_eq!(file, tree::stored(&cache.children), "the cache's kept tree is the file's, ids and all, after {after}");
    }
}

/// `writes` random writes, the kept tree held to the oracle after each;
/// then a reopen.
async fn walk(store: Store, path: &Path, seed: u64, writes: usize, epics: &[&str]) -> Store {
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    for step in 0..writes {
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        assert_tree_is_a_scan(&store, Some(path), &format!("step {step}: {what}"));
    }
    assert!(tally.moves > 0 && tally.created > 0 && tally.crossings > 0 && tally.kinds > 0, "{tally:?}");
    let before = kept(&store);
    drop(store);
    let store = Store::open(path).unwrap();
    assert_eq!(kept(&store), before, "a reopen reads what was kept");
    assert_tree_is_a_scan(&store, Some(path), "the reopen");
    store
}

/// How deep the deepest ticket is, and how many lists are bitmaps — that the
/// walk made the shapes it is meant to test.
fn shape(store: &Store) -> (usize, usize) {
    let rows = rows(store);
    let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
    let depth = rows.iter().map(|t| ancestors_of(&by_id, t).len()).max().unwrap_or(0);
    let cache = store.tickets.read().unwrap();
    let many = cache.children.every().flat_map(|(_, k)| k.lists()).filter(|(_, s)| s.many().is_some()).count();
    (depth, many)
}

#[tokio::test]
async fn the_kept_tree_is_a_scan_after_every_write_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "a fresh open");
    let store = walk(store, &path, 0x7EE_0013, 450, &["AA", "MAP", "SUB", "ZED"]).await;
    let (depth, many) = shape(&store);
    assert!(depth >= 3 && many > 0, "nested ({depth} deep) with lists kept as bitmaps ({many})");
}

#[tokio::test]
async fn the_kept_tree_is_a_scan_after_every_write_on_a_nested_seed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic_shaped(SYS, SeedSpec { count: 1_500, seed: 13, into_existing: false }, SeedShape::Nested).unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "a nested seed");
    let (depth, many) = shape(&store);
    assert!(depth == 3 && many > 10, "root → execution → build → doc ({depth} deep), {many} bitmaps");
    let epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).take(4).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    let store = walk(store, &path, 0x5EED_0013, 250, &epics).await;
    store.seed_synthetic_shaped(SYS, SeedSpec { count: 500, seed: 14, into_existing: true }, SeedShape::Nested).unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "a nested seed into the walked store");
}

#[tokio::test]
async fn the_kept_tree_is_a_scan_after_every_write_on_each_fixture() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        let store = Store::open(&path).unwrap();
        assert_tree_is_a_scan(&store, Some(&path), &format!("the first open of {name}"));
        let mut epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        walk(store, &path, 0x13F1 + i as u64, 120, &epics).await;
    }
}

/// What only a hand edit makes — parent cycles inside a shortcode and across
/// two and three, a parent that names no ticket — and the writes that
/// repair it: a move off a cycle builds the tree again; the birth of the id
/// a dangling parent names counts the subtree that was waiting for it in
/// its new ancestors.
#[tokio::test]
async fn the_kept_tree_is_a_scan_on_hand_edited_shapes_and_through_their_repairs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    tests::write_db(&path, &tests::legacy());
    let store = Store::open(&path).unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "the first open");
    let (tallies, _) = kept(&store);
    assert!(tallies.keys().any(|(p, _, _)| p == "GHOST-1"), "a dangling parent's subtree is tallied under its name");
    assert!(tallies.keys().any(|(p, _, _)| p == "CYC-1") && tallies.keys().any(|(p, _, _)| p == "CYC-2"), "both ends of a cycle");

    store.set_parent(SYS.moving("CYC-2", "CYC-4")).await.unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "a move off a cycle (a rebuild)");
    store.set_parent(SYS.moving("CXA-1", "PLEX-2")).await.unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "a move off the cycle across three shortcodes");
    store.create(SYS, new_ticket("GHOST", Kind::Scope, None, Vec::new())).await.unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "the birth a dangling parent named");
    walk(
        store,
        &path,
        0x4A4D_0013,
        300,
        &["AAA", "AGS", "BBB", "CYC", "DEMO", "NOSC", "PLEX", "TRAN", "XPC", "DUA", "GHOST", "NEW"],
    )
    .await;
}

/// A list crossing the threshold both ways, by births and by moves, in one
/// place: its holders are built when it becomes a bitmap and dropped when it
/// becomes a vector again, and a re-parent moves a subtree's whole tally.
#[tokio::test]
async fn a_list_across_the_threshold_and_a_subtree_moved_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cross.db");
    let store = Store::open(&path).unwrap();
    let mk = |kind: Kind, parent: Option<&str>| new_ticket("TT", kind, parent.map(String::from), Vec::new());
    store.create(SYS, mk(Kind::Scope, None)).await.unwrap(); // TT-1
    for _ in 0..3 {
        store.create(SYS, mk(Kind::Execution, Some("TT-1"))).await.unwrap(); // TT-2..4
    }
    for _ in 0..(HOLD_OVER + 2) {
        store.create(SYS, mk(Kind::Build, Some("TT-2"))).await.unwrap();
        assert_tree_is_a_scan(&store, Some(&path), "a birth under TT-2");
    }
    store.create(SYS, mk(Kind::Doc, Some("TT-5"))).await.unwrap();
    store.create(SYS, mk(Kind::Doc, Some("TT-6"))).await.unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "docs under two builds");
    // TT-2's list is a bitmap whose holders name TT-5 and TT-6.
    let (_, holders) = kept(&store);
    assert!(holders.iter().any(|((p, _, _, _), seqs)| p == "TT-2" && seqs == &vec![5, 6]), "{holders:?}");
    // Move builds out of TT-2 until it is a vector again, and back.
    let moved: Vec<String> = (5..5 + HOLD_OVER as u32).map(|s| format!("TT-{s}")).collect();
    for id in &moved {
        store.set_parent(SYS.moving(id, "TT-3")).await.unwrap();
        assert_tree_is_a_scan(&store, Some(&path), &format!("{id} to TT-3"));
    }
    for id in &moved {
        store.set_parent(SYS.moving(id, "TT-2")).await.unwrap();
        assert_tree_is_a_scan(&store, Some(&path), &format!("{id} back to TT-2"));
    }
    // A subtree moved whole: TT-3 under TT-4, then a status change deep in it.
    store.set_parent(SYS.moving("TT-3", "TT-4")).await.unwrap();
    store.set_parent(SYS.moving("TT-2", "TT-3")).await.unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "TT-2's subtree under TT-3 under TT-4");
    store.set_status(SYS.permit("TT-6"), Status::Complete).await.unwrap();
    store.link_commits(SYS.permit("TT-5"), vec!["abc1234".into()]).await.unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "cell moves four deep");
    let (tallies, _) = kept(&store);
    let below = |p: &str| tallies.iter().filter(|((x, _, _), _)| x == p).map(|(_, n)| *n).sum::<u32>();
    assert_eq!(below("TT-4"), below("TT-3") + 1);
    assert_eq!(below("TT-1") as usize, rows(&store).len() - 1, "TT-1 holds every other ticket");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_tree_is_a_scan(&store, Some(&path), "the reopen");
}

// ---------------------------------------------------------------------------
// `tree`, and `under` on every filtered read, against the rule.

use super::tests_filtered::{assert_lens_is_a_scan, Plain, HISTORY_VOCAB};
use super::tests_language::{filter_names, random_filter};
use crate::filter::Filter;

/// One `tree` page: each row's id, `matched` and `below`; `next`;
/// `remaining`.
type TreeAnswer = (Vec<(String, bool, u64)>, Option<String>, u64);

/// A random filter, half the time with an `under` of its own beside it.
fn tree_filter(rng: &mut Rng, names: &[String]) -> Filter {
    // MENGER-13: a quarter of the time a history group beside it — drawn from a
    // generator of its own, so the draws below are the ones they were.
    let mut hrng = Rng(rng.0 ^ 0x4E11_6E13);
    let f = random_filter(rng, names);
    let f = if hrng.below(4) == 0 { f.and(&history_group(&mut hrng)) } else { f };
    if rng.below(2) == 0 {
        let x = names[rng.below(names.len())].to_ascii_uppercase();
        let not = if rng.below(4) == 0 { "not " } else { "" };
        return f.and(&Filter::parse(&format!("{not}under {x}")).unwrap());
    }
    f
}

/// MENGER-13: one group over [`HISTORY_VOCAB`] — a predicate, its `not`, or
/// two of them joined by `or`.
pub(super) fn history_group(rng: &mut Rng) -> Filter {
    let pick = |rng: &mut Rng| {
        let not = if rng.below(3) == 0 { "not " } else { "" };
        format!("{not}{}", HISTORY_VOCAB[rng.below(HISTORY_VOCAB.len())])
    };
    let text = match rng.below(3) {
        0 => format!("({} or {})", pick(rng), pick(rng)),
        _ => pick(rng),
    };
    Filter::parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

/// `tree` by the rule, from the rows alone: the rows at the address — the
/// parent's children (by their `parent` field), or the shortcode's root
/// (its lowest-seq ticket of a kind that roots one), or every ticket of a
/// shortcode with no root — in the store's order; those that match or have
/// a match among the tickets whose lineage holds them; paged after a
/// position.
#[allow(clippy::too_many_arguments)]
fn tree_by_rule(
    plain: &Plain,
    filter: &Filter,
    parent: Option<&str>,
    epic: Option<&str>,
    after: Option<&str>,
    limit: Option<u32>,
) -> Result<TreeAnswer, String> {
    let rows = &plain.rows;
    let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
    let matches: Vec<bool> = rows.iter().zip(&plain.cells).map(|(t, k)| filter.matches(&plain.facts(t, *k))).collect();
    let mut below: HashMap<String, u64> = HashMap::new();
    for (t, _) in rows.iter().zip(&matches).filter(|(_, m)| **m) {
        for a in ancestors_of(&by_id, t) {
            *below.entry(a).or_insert(0) += 1;
        }
    }
    let address: Vec<usize> = match (parent.map(|p| p.trim().to_ascii_uppercase()), epic.map(|e| e.trim().to_ascii_uppercase())) {
        (Some(p), None) => (0..rows.len()).filter(|i| rows[*i].parent.as_deref() == Some(p.as_str())).collect(),
        (None, Some(e)) => {
            let mine: Vec<usize> = (0..rows.len()).filter(|i| rows[*i].epic == e).collect();
            match mine.iter().copied().filter(|i| rows[*i].kind.roots_a_shortcode()).min_by_key(|i| rows[*i].seq) {
                Some(root) => vec![root],
                None => mine,
            }
        }
        _ => return Err("tree: give `parent` or `epic`".into()),
    };
    let shown: Vec<usize> = address.into_iter().filter(|i| matches[*i] || below.get(rows[*i].id.as_str()).copied().unwrap_or(0) > 0).collect();
    let after = after.map(|a| a.trim().to_ascii_uppercase());
    let from = match after.as_deref() {
        None => 0,
        Some(id) => {
            let Some(c) = rows.iter().position(|t| t.id == id) else { return Err(format!("after: no ticket {id}")) };
            let key = OrderKey::of(&rows[c]);
            shown.iter().filter(|i| OrderKey::of(&rows[**i]) <= key).count()
        }
    };
    let to = limit.map_or(shown.len(), |n| (from + n as usize).min(shown.len()));
    let page: Vec<(String, bool, u64)> = shown[from..to]
        .iter()
        .map(|i| (rows[*i].id.to_string(), matches[*i], below.get(rows[*i].id.as_str()).copied().unwrap_or(0)))
        .collect();
    let remaining = (shown.len() - to) as u64;
    let next = if remaining == 0 { None } else { page.last().map(|p| p.0.clone()).or(after) };
    Ok((page, next, remaining))
}

/// FILTER-23: where a hosted shortcode's row is drawn in the nav that shows
/// only matches, by the rule: under a ticket, or at the top of a shortcode.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Placed {
    Ticket(String),
    Top(String),
}

/// FILTER-23: `tree { matches_only }` by the rule, from the rows alone and
/// each shortcode's host as the roll-ups settle it (MNG-14, held to plan()
/// by their own tests). A ticket's nearest matches: walking down by the
/// `parent` field from it, each child that matches, and in place of a child
/// that does not but has a match among the tickets whose lineage holds it,
/// that child's own — each ticket once; a hosted shortcode's root is never
/// one of them, nor gone below. A hosted shortcode is placed from its host
/// up at the first ticket that matches or is its shortcode's root (lowest
/// seq of a kind that roots one); past a ticket with no parent, at the top
/// of its shortcode; nowhere if the chain names no ticket or comes back. At
/// a parent: its nearest matches in the store's order, then the hosted
/// shortcodes placed there, in shortcode order, as their roots. At a
/// shortcode: its root if it matches, else the root's answer; with no root,
/// its tickets that match, then those placed at its top. Paged: a cursor
/// that is one of the placed roots is a place among them, any other ticket
/// a place in the store's order among the matches.
#[allow(clippy::too_many_arguments)]
fn tree_matches_by_rule(
    store: &Store,
    plain: &Plain,
    filter: &Filter,
    parent: Option<&str>,
    epic: Option<&str>,
    after: Option<&str>,
    limit: Option<u32>,
) -> Result<TreeAnswer, String> {
    tree_kept_by_rule(store, plain, filter, parent, epic, after, limit, None)
}

/// FILTER-24: [`tree_matches_by_rule`] with `keep` — the answer as it would
/// be if that ticket matched: everything above read with it a match, each
/// row's `matched` and `below` still the truth. A ticket that roots its
/// shortcode (its shortcode's row already), matches, names no ticket, or
/// whose chain comes back to it (a hand edit) keeps nothing.
#[allow(clippy::too_many_arguments)]
fn tree_kept_by_rule(
    store: &Store,
    plain: &Plain,
    filter: &Filter,
    parent: Option<&str>,
    epic: Option<&str>,
    after: Option<&str>,
    limit: Option<u32>,
    keep: Option<&str>,
) -> Result<TreeAnswer, String> {
    let rows = &plain.rows;
    let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
    let at: HashMap<&str, usize> = rows.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
    let real: Vec<bool> = rows.iter().zip(&plain.cells).map(|(t, k)| filter.matches(&plain.facts(t, *k))).collect();
    let root_of = |e: &str| (0..rows.len()).filter(|i| rows[*i].epic == e && rows[*i].kind.roots_a_shortcode()).min_by_key(|i| rows[*i].seq);
    let comes_back = |i: usize| {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut cur = rows[i].parent.as_deref();
        while let Some(p) = cur {
            if p == rows[i].id {
                return true;
            }
            if !seen.insert(p) {
                return false;
            }
            cur = by_id.get(p).and_then(|r| r.parent.as_deref());
        }
        false
    };
    let kept = keep
        .map(|k| k.trim().to_ascii_uppercase())
        .and_then(|k| at.get(k.as_str()).copied())
        .filter(|i| !real[*i] && root_of(&rows[*i].epic) != Some(*i) && !comes_back(*i));
    let matches: Vec<bool> = (0..rows.len()).map(|i| real[i] || kept == Some(i)).collect();
    let below_of = |m: &[bool]| {
        let mut below: HashMap<String, u64> = HashMap::new();
        for (t, _) in rows.iter().zip(m).filter(|(_, m)| **m) {
            for a in ancestors_of(&by_id, t) {
                *below.entry(a).or_insert(0) += 1;
            }
        }
        below
    };
    let (below, read) = (below_of(&real), below_of(&matches));
    let holds = |i: usize| read.get(rows[i].id.as_str()).copied().unwrap_or(0) > 0;
    let hosts: BTreeMap<String, String> = {
        let cache = store.tickets.read().unwrap();
        let epics: std::collections::BTreeSet<&str> = rows.iter().map(|t| t.epic.as_str()).collect();
        epics.into_iter().filter_map(|e| cache.rollups.host(&ShortcodeId::from(e)).map(|h| (e.to_string(), h.to_string()))).collect()
    };
    let hosted: HashSet<usize> = hosts.keys().filter_map(|e| root_of(e)).collect();
    let mut kids: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, t) in rows.iter().enumerate() {
        if let Some(p) = &t.parent {
            kids.entry(p.as_str()).or_default().push(i);
        }
    }
    let nearest = |p: &str| -> Vec<usize> {
        let mut out: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        let mut stack = vec![p.to_string()];
        let mut seen: HashSet<String> = HashSet::from([p.to_string()]);
        while let Some(x) = stack.pop() {
            for &c in kids.get(x.as_str()).into_iter().flatten() {
                if hosted.contains(&c) {
                    continue;
                }
                if matches[c] {
                    out.insert(c);
                } else if holds(c) && seen.insert(rows[c].id.to_string()) {
                    stack.push(rows[c].id.to_string());
                }
            }
        }
        let mut v: Vec<usize> = out.into_iter().collect();
        v.sort_by_key(|i| OrderKey::of(&rows[*i]));
        v
    };
    let place = |host: &str| -> Option<Placed> {
        let mut cur = host.to_string();
        let mut seen: HashSet<String> = HashSet::new();
        loop {
            let i = *at.get(cur.as_str())?;
            if !seen.insert(cur.clone()) {
                return None;
            }
            let t = &rows[i];
            if root_of(t.epic.as_str()) == Some(i) || matches[i] {
                return Some(Placed::Ticket(t.id.to_string()));
            }
            match &t.parent {
                None => return Some(Placed::Top(t.epic.to_string())),
                Some(p) => cur = p.to_string(),
            }
        }
    };
    let tail = |here: Placed| -> Vec<usize> { hosts.iter().filter(|(_, h)| place(h).as_ref() == Some(&here)).filter_map(|(e, _)| root_of(e)).collect() };
    let (seg, tail) = match (parent.map(|p| p.trim().to_ascii_uppercase()), epic.map(|e| e.trim().to_ascii_uppercase())) {
        (Some(p), None) => (nearest(&p), tail(Placed::Ticket(p.clone()))),
        (None, Some(e)) => match root_of(&e) {
            Some(r) if matches[r] => (vec![r], Vec::new()),
            Some(r) => (nearest(&rows[r].id), tail(Placed::Ticket(rows[r].id.to_string()))),
            None => ((0..rows.len()).filter(|i| rows[*i].epic == e && matches[*i]).collect(), tail(Placed::Top(e.clone()))),
        },
        _ => return Err("tree: give `parent` or `epic`".into()),
    };
    let shown: Vec<usize> = seg.iter().chain(&tail).copied().collect();
    let after = after.map(|a| a.trim().to_ascii_uppercase());
    let from = match after.as_deref() {
        None => 0,
        Some(id) => {
            let Some(c) = rows.iter().position(|t| t.id == id) else { return Err(format!("after: no ticket {id}")) };
            match tail.iter().position(|i| *i == c) {
                Some(k) => seg.len() + k + 1,
                None => {
                    let key = OrderKey::of(&rows[c]);
                    seg.iter().filter(|i| OrderKey::of(&rows[**i]) <= key).count()
                }
            }
        }
    };
    let to = limit.map_or(shown.len(), |n| (from + n as usize).min(shown.len()));
    let page: Vec<(String, bool, u64)> = shown[from..to]
        .iter()
        .map(|i| (rows[*i].id.to_string(), real[*i], below.get(rows[*i].id.as_str()).copied().unwrap_or(0)))
        .collect();
    let remaining = (shown.len() - to) as u64;
    let next = if remaining == 0 { None } else { page.last().map(|p| p.0.clone()).or(after) };
    Ok((page, next, remaining))
}

fn tree_answered(store: &Store, filter: &Filter, parent: Option<&str>, epic: Option<&str>, after: Option<&str>, limit: Option<u32>) -> Result<TreeAnswer, String> {
    tree_answered_in(store, filter, parent, epic, after, limit, false)
}

/// `tree`'s answer — FILTER-23: in the nav that shows only matches when
/// `matches_only`.
fn tree_answered_in(
    store: &Store,
    filter: &Filter,
    parent: Option<&str>,
    epic: Option<&str>,
    after: Option<&str>,
    limit: Option<u32>,
    matches_only: bool,
) -> Result<TreeAnswer, String> {
    tree_asked(store, filter, parent, epic, after, limit, matches_only, None)
}

/// FILTER-24: `tree { matches_only, keep }`'s answer.
fn tree_kept_in(store: &Store, filter: &Filter, parent: Option<&str>, epic: Option<&str>, after: Option<&str>, limit: Option<u32>, keep: &str) -> Result<TreeAnswer, String> {
    tree_asked(store, filter, parent, epic, after, limit, true, Some(keep))
}

#[allow(clippy::too_many_arguments)]
fn tree_asked(
    store: &Store,
    filter: &Filter,
    parent: Option<&str>,
    epic: Option<&str>,
    after: Option<&str>,
    limit: Option<u32>,
    matches_only: bool,
    keep: Option<&str>,
) -> Result<TreeAnswer, String> {
    let q = TreeQuery { parent: parent.map(String::from), epic: epic.map(String::from), lens: Lens::of(filter).unwrap(), matches_only, keep: keep.map(String::from) };
    store
        .tree_page(SYS, &q, after, limit)
        .map(|p| (p.nodes.into_iter().map(|n| (n.summary.id.into_string(), n.matched, n.below)).collect(), p.next, p.remaining))
        .map_err(|e| e.to_string())
}

/// What the tree reads exercised.
#[derive(Debug, Default)]
struct TreeSeen {
    matched: usize,
    holding: usize,
    pages: usize,
    with_under: usize,
    /// FILTER-23: pages of the nav that shows only matches; the rows on
    /// them drawn away from their parent (moved up); the hosted shortcodes'
    /// roots after them.
    only_pages: usize,
    moved_up: usize,
    placed: usize,
    /// FILTER-24: pages asked with a `keep` on its way up, and the kept
    /// rows they drew (a ticket that does not match).
    kept_pages: usize,
    kept_rows: usize,
}

/// Random addresses under random filters (half with an `under` of their
/// own): every page after a random cursor, at a random limit, equal to the
/// rule; then a walk by `next` that is the whole answer. And the same
/// filters' `counts` and `list` pages (FILTER-4's oracle, with lineage).
fn assert_tree_reads_are_a_scan(store: &Store, rng: &mut Rng, seen: &mut TreeSeen, what: &str) {
    let plain = Plain::of(store);
    let names = filter_names(store);
    let rows = &plain.rows;
    if rows.is_empty() {
        return;
    }
    let parents: Vec<String> = {
        let p: std::collections::BTreeSet<&str> = rows.iter().filter_map(|t| t.parent.as_deref()).collect();
        p.into_iter().map(String::from).collect()
    };
    let epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).collect();
    for _ in 0..6 {
        let filter = tree_filter(rng, &names);
        seen.with_under += usize::from(filter.format().contains("under"));
        for _ in 0..5 {
            let (parent, epic) = match rng.below(12) {
                0..=5 if !parents.is_empty() => (Some(parents[rng.below(parents.len())].clone()), None),
                6 => (Some(rows[rng.below(rows.len())].id.to_string()), None),
                7 => (Some("NONE-1".to_string()), None),
                8 if !epics.is_empty() => (None, Some(epics[rng.below(epics.len())].to_ascii_lowercase())),
                9 => (Some(rows[0].id.to_string()), Some(rows[0].epic.to_string())),
                _ if !epics.is_empty() => (None, Some(epics[rng.below(epics.len())].clone())),
                _ => (None, None),
            };
            let after = match rng.below(4) {
                0 => Some(rows[rng.below(rows.len())].id.to_string()),
                1 => Some("NO-SUCH-1".to_string()),
                _ => None,
            };
            let limit = [None, Some(0), Some(1), Some(3), Some(50)][rng.below(5)];
            let want = tree_by_rule(&plain, &filter, parent.as_deref(), epic.as_deref(), after.as_deref(), limit);
            let got = tree_answered(store, &filter, parent.as_deref(), epic.as_deref(), after.as_deref(), limit);
            match (&got, &want) {
                (Ok(g), Ok(w)) => {
                    assert_eq!(g, w, "{what}: tree {parent:?} {epic:?} under `{}` after {after:?} limit {limit:?}", filter.format());
                    seen.pages += 1;
                    seen.matched += g.0.iter().filter(|r| r.1).count();
                    seen.holding += g.0.iter().filter(|r| !r.1 && r.2 > 0).count();
                }
                (Err(_), Err(_)) => {}
                _ => panic!("{what}: tree {parent:?} {epic:?} under `{}` after {after:?}: {got:?} vs {want:?}", filter.format()),
            }
            // FILTER-23: the same address, page and filter, only matches.
            let want = tree_matches_by_rule(store, &plain, &filter, parent.as_deref(), epic.as_deref(), after.as_deref(), limit);
            let got = tree_answered_in(store, &filter, parent.as_deref(), epic.as_deref(), after.as_deref(), limit, true);
            match (&got, &want) {
                (Ok(g), Ok(w)) => {
                    assert_eq!(g, w, "{what}: tree {parent:?} {epic:?} matches only under `{}` after {after:?} limit {limit:?}", filter.format());
                    seen.only_pages += 1;
                    count_moved(&plain, g, parent.as_deref(), seen);
                }
                (Err(_), Err(_)) => {}
                _ => panic!("{what}: tree {parent:?} {epic:?} matches only under `{}` after {after:?}: {got:?} vs {want:?}", filter.format()),
            }
        }
        // FILTER-24: a kept ticket, asked about at every address on its way
        // up — its ancestors, its shortcode, itself. Drawn from a generator
        // of its own, so the reads above see what they saw before it.
        let mut krng = Rng(rng.0 ^ 0xF1_17E2_0024);
        for _ in 0..2 {
            let k = &rows[krng.below(rows.len())];
            let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
            let mut addresses: Vec<(Option<String>, Option<String>)> =
                ancestors_of(&by_id, k).into_iter().take(8).map(|a| (Some(a), None)).collect();
            addresses.push((None, Some(k.epic.to_string())));
            addresses.push((Some(k.id.to_string()), None));
            for (parent, epic) in addresses {
                let after = match krng.below(4) {
                    0 => Some(rows[krng.below(rows.len())].id.clone()),
                    1 => Some(k.id.clone()),
                    _ => None,
                };
                let limit = [None, Some(0), Some(1), Some(3), Some(50)][krng.below(5)];
                let want = tree_kept_by_rule(store, &plain, &filter, parent.as_deref(), epic.as_deref(), after.as_deref(), limit, Some(&k.id));
                let got = tree_kept_in(store, &filter, parent.as_deref(), epic.as_deref(), after.as_deref(), limit, &k.id);
                match (&got, &want) {
                    (Ok(g), Ok(w)) => {
                        assert_eq!(g, w, "{what}: tree {parent:?} {epic:?} keeping {} under `{}` after {after:?} limit {limit:?}", k.id, filter.format());
                        seen.kept_pages += 1;
                        seen.kept_rows += g.0.iter().filter(|r| r.0 == k.id && !r.1).count();
                    }
                    (Err(_), Err(_)) => {}
                    _ => panic!("{what}: tree {parent:?} {epic:?} keeping {} under `{}` after {after:?}: {got:?} vs {want:?}", k.id, filter.format()),
                }
            }
        }
        // A walk by `next` is the whole answer.
        if let Some(p) = parents.get(rng.below(parents.len().max(1))) {
            let whole = tree_by_rule(&plain, &filter, Some(p), None, None, None).unwrap().0;
            let (mut after, mut walked) = (None::<String>, Vec::new());
            loop {
                let (page, next, _) = tree_answered(store, &filter, Some(p), None, after.as_deref(), Some(2)).unwrap();
                walked.extend(page);
                match next {
                    Some(n) => after = Some(n),
                    None => break,
                }
            }
            if rows.iter().all(|t| t.id == t.id.to_ascii_uppercase()) {
                assert_eq!(walked, whole, "{what}: a walk of {p}'s tree under `{}`", filter.format());
            }
            // FILTER-23: and of the nav that shows only matches, the moved-up
            // matches and the placed shortcodes paged in their new place.
            let whole = tree_matches_by_rule(store, &plain, &filter, Some(p), None, None, None).unwrap().0;
            let (mut after, mut walked) = (None::<String>, Vec::new());
            loop {
                let (page, next, _) = tree_answered_in(store, &filter, Some(p), None, after.as_deref(), Some(2), true).unwrap();
                walked.extend(page);
                match next {
                    Some(n) => after = Some(n),
                    None => break,
                }
            }
            if rows.iter().all(|t| t.id == t.id.to_ascii_uppercase()) {
                assert_eq!(walked, whole, "{what}: a walk of {p}'s matches under `{}`", filter.format());
            }
        }
        // `under` in FILTER-4's reads: counts and pages, by the same oracle.
        let lens = Lens::of(&filter).unwrap();
        assert_lens_is_a_scan(store, &plain, &lens, &|t: &Row, k: Key| filter.matches(&plain.facts(t, k)), rng, what);
    }
}

/// FILTER-23: what a page of the nav that shows only matches exercised —
/// its rows whose parent is not the address (moved up), and the rows that
/// do not match (the placed shortcodes' roots).
fn count_moved(plain: &Plain, page: &TreeAnswer, parent: Option<&str>, seen: &mut TreeSeen) {
    let Some(p) = parent.map(|p| p.trim().to_ascii_uppercase()) else { return };
    for (id, matched, _) in &page.0 {
        let row = plain.rows.iter().find(|t| t.id == *id);
        if !matched {
            seen.placed += 1;
        } else if row.and_then(|t| t.parent.as_deref()) != Some(p.as_str()) {
            seen.moved_up += 1;
        }
    }
}

async fn tree_walk(store: Store, path: &Path, seed: u64, writes: usize, every: usize, epics: &[&str]) -> TreeSeen {
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    let mut seen = TreeSeen::default();
    assert_tree_reads_are_a_scan(&store, &mut rng, &mut seen, "the start");
    for step in 0..writes {
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        if step % every == every - 1 {
            assert_tree_reads_are_a_scan(&store, &mut rng, &mut seen, &format!("step {step}: {what}"));
        }
    }
    drop(store);
    let store = Store::open(path).unwrap();
    for _ in 0..3 {
        assert_tree_reads_are_a_scan(&store, &mut rng, &mut seen, "the reopen");
    }
    seen
}

#[tokio::test]
async fn tree_and_under_equal_the_rule_on_a_nested_seed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic_shaped(SYS, SeedSpec { count: 1_200, seed: 131, into_existing: false }, SeedShape::Nested).unwrap();
    let epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).take(4).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    // The seed only steers coverage (the answers are checked whatever it
    // is); SDLC-15's eleventh status reshuffled the walk, and A133 is a
    // walk that still draws matches away from their parent often enough —
    // FILTER-27's six history predicates reshuffled it again: A137; FILTER-30's two `touched` ones: A139; ORG-3's walks that file no namesake: A13A.
    let seen = tree_walk(store, &path, 0x7EE_A13A, 240, 12, &epics).await;
    assert!(seen.pages > 300 && seen.matched > 100 && seen.holding > 10 && seen.with_under > 40, "{seen:?}");
    assert!(seen.only_pages > 300 && seen.moved_up > 20, "{seen:?}");
    assert!(seen.kept_pages > 300 && seen.kept_rows > 20, "{seen:?}");
}

/// Every parent and every shortcode of a nested seed, whole answers, under
/// FILTER-3 §6.2's nine filters and filters with `under` — after writes of
/// every kind.
#[tokio::test]
async fn every_address_of_a_nested_seed_answers_as_the_rule_does() {
    let store = Store::new();
    store.seed_synthetic_shaped(SYS, SeedSpec { count: 900, seed: 6, into_existing: false }, SeedShape::Nested).unwrap();
    let epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).take(3).collect();
    let epic_refs: Vec<&str> = epics.iter().map(String::as_str).collect();
    let mut rng = Rng(0x5EE_E13);
    let mut tally = Tally::default();
    let (mut matched, mut holding) = (0, 0);
    let (mut moved, mut tops) = (0, 0);
    for round in 0..3 {
        let plain = Plain::of(&store);
        let parents: std::collections::BTreeSet<String> = plain.rows.iter().filter_map(|t| t.parent.as_deref().map(String::from)).collect();
        let big = plain.rows.iter().find(|t| t.kind == Kind::Execution && parents.contains(t.id.as_str())).map(|t| t.id.clone()).unwrap();
        let root = plain.rows[0].id.clone();
        let mut texts: Vec<String> = [
            "status Ready",
            "status in (Pending, Ready) and kind build",
            "ready",
            "blocked and severity critical",
            "kind scope and status Idea and severity critical",
            "category in (done, dropped)",
            "(status Blocked or blocked) and not kind doc",
            "not kind doc",
            "category in (active, review)",
            "kind doc",
        ]
        .map(String::from)
        .to_vec();
        texts.extend([format!("under {big}"), format!("ready and under {big}"), format!("not under {big} and kind doc"), format!("under {root} and not under {big}")]);
        texts.push(format!("under {}", epics[0]));
        for text in &texts {
            let filter = Filter::parse(text).unwrap();
            for p in &parents {
                let want = tree_by_rule(&plain, &filter, Some(p), None, None, None);
                let got = tree_answered(&store, &filter, Some(p), None, None, None);
                assert_eq!(got, want, "round {round}: {p} under `{text}`");
                let rows = got.unwrap().0;
                matched += rows.iter().filter(|r| r.1).count();
                holding += rows.iter().filter(|r| !r.1 && r.2 > 0).count();
            }
            for e in store.extents_text().into_iter().map(|(e, _)| e) {
                assert_eq!(tree_answered(&store, &filter, None, Some(&e), None, None), tree_by_rule(&plain, &filter, None, Some(&e), None, None), "{e} `{text}`");
            }
            // FILTER-23: every address again, in the nav that shows only matches.
            for p in &parents {
                let want = tree_matches_by_rule(&store, &plain, &filter, Some(p), None, None, None);
                let got = tree_answered_in(&store, &filter, Some(p), None, None, None, true);
                assert_eq!(got, want, "round {round}: {p} matches only under `{text}`");
                let rows = got.unwrap().0;
                assert!(rows.iter().all(|r| r.1), "a nested seed hosts nothing: every row matches");
                moved += rows.iter().filter(|r| plain.rows.iter().find(|t| t.id == r.0).and_then(|t| t.parent.as_deref()) != Some(p.as_str())).count();
            }
            for e in store.extents_text().into_iter().map(|(e, _)| e) {
                let want = tree_matches_by_rule(&store, &plain, &filter, None, Some(&e), None, None);
                assert_eq!(tree_answered_in(&store, &filter, None, Some(&e), None, None, true), want, "{e} matches only `{text}`");
                tops += want.unwrap().0.len();
            }
        }
        for _ in 0..60 {
            random_write(&store, &mut rng, &epic_refs, &mut tally).await;
        }
    }
    assert!(matched > 1_000 && holding > 300, "{matched} matching rows, {holding} holding only");
    assert!(moved > 300 && tops > 100, "{moved} matches drawn above their parent, {tops} rows at the shortcodes' tops");
}

#[tokio::test]
async fn tree_and_under_equal_the_rule_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    let seen = tree_walk(store, &path, 0x7EE_C13, 360, 15, &["AA", "MAP", "SUB", "ZED"]).await;
    assert!(seen.pages > 300 && seen.holding > 5, "{seen:?}");
}

#[tokio::test]
async fn tree_and_under_equal_the_rule_on_each_fixture() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        let store = Store::open(&path).unwrap();
        let mut epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        tree_walk(store, &path, 0x13F_0 + i as u64, 100, 20, &epics).await;
    }
}

/// Hand-edited shapes: parent cycles (whose members' subtrees hold their
/// own ancestors — answered by looking), a dangling parent, a legacy
/// shortcode with no root (its tickets flat), sub-shortcodes hosted under
/// tickets of others (`under` a shortcode reaches what it hosts).
#[tokio::test]
async fn tree_and_under_equal_the_rule_on_hand_edited_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    tests::write_db(&path, &tests::legacy());
    let store = Store::open(&path).unwrap();
    let mut rng = Rng(0x1E6A_C13);
    let mut seen = TreeSeen::default();
    for _ in 0..12 {
        assert_tree_reads_are_a_scan(&store, &mut rng, &mut seen, "the legacy shapes");
    }
    // The shapes, by name.
    let plain = Plain::of(&store);
    for (parent, epic) in [(Some("CYC-1"), None), (Some("CYC-2"), None), (Some("CXA-1"), None), (Some("GHOST-1"), None), (None, Some("NOSC")), (None, Some("DEMO")), (Some("PLEX-1"), None)] {
        for text in ["", "under PLEX", "under CYC-1", "status Complete", "not under GHOST-1", "under CXB and status Pending"] {
            let filter = Filter::parse(text).unwrap();
            assert_eq!(
                tree_answered(&store, &filter, parent, epic, None, None),
                tree_by_rule(&plain, &filter, parent, epic, None, None),
                "{parent:?} {epic:?} `{text}`"
            );
            assert_eq!(
                tree_answered_in(&store, &filter, parent, epic, None, None, true),
                tree_matches_by_rule(&store, &plain, &filter, parent, epic, None, None),
                "{parent:?} {epic:?} matches only `{text}`"
            );
        }
    }
    let tran = tree_answered(&store, &Filter::parse("under PLEX").unwrap(), Some("PLEX-1"), None, None, None).unwrap();
    assert!(tran.0.iter().any(|(id, m, _)| id == "TRAN-1" && *m), "TRAN is hosted by PLEX: under PLEX holds there: {tran:?}");
    let mut tally = Tally::default();
    for step in 0..120 {
        let what = random_write(&store, &mut rng, &["AGS", "CYC", "DEMO", "NOSC", "PLEX", "TRAN", "XPC", "GHOST", "NEW"], &mut tally).await;
        if step % 10 == 9 {
            assert_tree_reads_are_a_scan(&store, &mut rng, &mut seen, &format!("step {step}: {what}"));
        }
    }
    assert!(seen.pages > 100, "{seen:?}");
}

/// FILTER-23, the ticket's examples: in the nav that shows only matches a
/// ticket that does not match is not a row — the matches below it are
/// drawn under its nearest matching ancestor, or at the top of their
/// shortcode; matches below a match stay under it; a hosted shortcode's row
/// is drawn where its host would be, or moves up the same way; and a write
/// that makes a ticket match or stop matching moves them back.
#[tokio::test]
async fn only_matches_move_up_to_the_nearest_match_or_the_shortcodes_top() {
    let store = Store::new();
    let mk = |epic: &str, kind: Kind, parent: Option<&str>| new_ticket(epic, kind, parent.map(String::from), Vec::new());
    for (epic, kind, parent) in [
        ("MENGER", Kind::Scope, None),            // MENGER-1, the root
        ("MENGER", Kind::Execution, Some("MENGER-1")), // MENGER-2, an area
        ("MENGER", Kind::Build, Some("MENGER-2")), // MENGER-3
        ("FILTER", Kind::Scope, Some("MENGER-2")), // FILTER-1, hosted at MENGER-2
        ("FILTER", Kind::Build, Some("FILTER-1")), // FILTER-2
        ("FILTER", Kind::Execution, Some("FILTER-1")), // FILTER-3
        ("FILTER", Kind::Build, Some("FILTER-3")), // FILTER-4
        ("PLEXUS", Kind::Scope, None),            // PLEXUS-1
        ("RETURNS", Kind::Scope, Some("PLEXUS-1")), // RETURNS-1, hosted at PLEXUS's root
        ("RETURNS", Kind::Build, Some("RETURNS-1")), // RETURNS-2 (the example's RETURNS-14)
        ("RETURNS", Kind::Execution, Some("RETURNS-1")), // RETURNS-3
        ("RETURNS", Kind::Build, Some("RETURNS-3")), // RETURNS-4
        ("RETURNS", Kind::Build, Some("RETURNS-4")), // RETURNS-5
    ] {
        store.create(SYS, mk(epic, kind, parent)).await.unwrap();
    }
    store.set_status(SYS.permit("RETURNS-1"), Status::Pending).await.unwrap();
    store.set_status(SYS.permit("RETURNS-3"), Status::Ready).await.unwrap();
    let ask = |text: &str, parent: Option<&str>, epic: Option<&str>| -> Vec<(String, bool, u64)> {
        let filter = Filter::parse(text).unwrap();
        let got = tree_answered_in(&store, &filter, parent, epic, None, None, true);
        assert_eq!(got, tree_matches_by_rule(&store, &Plain::of(&store), &filter, parent, epic, None, None), "{parent:?} {epic:?} `{text}`");
        got.unwrap().0
    };
    let row = |id: &str, matched: bool, below: u64| (id.to_string(), matched, below);

    // `kind build`: the scopes are not rows; their builds move up.
    assert_eq!(ask("kind build", None, Some("RETURNS")), [row("RETURNS-2", true, 0), row("RETURNS-4", true, 1)], "under the heading RETURNS");
    assert_eq!(ask("kind build", Some("RETURNS-4"), None), [row("RETURNS-5", true, 0)], "a match below a match stays under it");
    assert_eq!(ask("kind build", Some("RETURNS-3"), None), [row("RETURNS-4", true, 1)]);
    assert_eq!(
        ask("kind build", None, Some("FILTER")),
        [row("FILTER-2", true, 0), row("FILTER-4", true, 0)],
        "FILTER as a heading: its builds, FILTER-4 up past FILTER-3"
    );
    assert_eq!(
        ask("kind build", None, Some("MENGER")),
        [row("MENGER-3", true, 0), row("FILTER-1", false, 2)],
        "MENGER-2 is not drawn: its build moves up, and FILTER's row with it"
    );
    assert_eq!(ask("kind build", None, Some("PLEXUS")), [row("RETURNS-1", false, 3)], "RETURNS's row, hosted at PLEXUS's root");
    assert_eq!(ask("kind build", Some("MENGER-2"), None), [row("MENGER-3", true, 0)], "MENGER-2's own address: FILTER is not drawn here");

    // `status Pending`: RETURNS-1 matches; RETURNS-2 stays under it; RETURNS-3
    // is Ready, so its Pending build moves up beside RETURNS-2.
    assert_eq!(ask("status Pending", None, Some("RETURNS")), [row("RETURNS-1", true, 3)]);
    assert_eq!(ask("status Pending", Some("RETURNS-1"), None), [row("RETURNS-2", true, 0), row("RETURNS-4", true, 1)]);

    // A scope filter: FILTER-1 matches and MENGER-2 (ORG-5: an execution,
    // as an area inside its shortcode is) does not, so FILTER's row moves up
    // to MENGER-1.
    assert_eq!(ask("kind scope", None, Some("MENGER")), [row("MENGER-1", true, 1)]);
    assert_eq!(ask("kind scope", Some("MENGER-1"), None), [row("FILTER-1", true, 0)]);
    assert_eq!(ask("kind scope", Some("MENGER-2"), None), []);

    // Paged in their new place: one row a page, across the matches and the
    // shortcodes drawn after them.
    let filter = Filter::parse("kind build").unwrap();
    let (mut after, mut walked) = (None::<String>, Vec::new());
    loop {
        let (page, next, _) = tree_answered_in(&store, &filter, None, Some("MENGER"), after.as_deref(), Some(1), true).unwrap();
        walked.extend(page.into_iter().map(|r| r.0));
        match next {
            Some(n) => after = Some(n),
            None => break,
        }
    }
    assert_eq!(walked, ["MENGER-3", "FILTER-1"]);
    let (page, next, remaining) = tree_answered_in(&store, &filter, Some("MENGER-1"), None, Some("FILTER-1"), None, true).unwrap();
    assert_eq!((page, next, remaining), (Vec::new(), None, 0), "after the last shortcode, nothing");

    // Live: MENGER-2 becomes a build — it is drawn, and holds what had moved
    // up past it; a build that stops matching lets its match move up.
    store.set_kind(SYS.permit("MENGER-2"), Kind::Build, None).await.unwrap();
    assert_eq!(ask("kind build", None, Some("MENGER")), [row("MENGER-2", true, 3)]);
    assert_eq!(ask("kind build", Some("MENGER-2"), None), [row("MENGER-3", true, 0), row("FILTER-1", false, 2)]);
    store.set_kind(SYS.permit("RETURNS-4"), Kind::Gap, None).await.unwrap();
    assert_eq!(ask("kind build", None, Some("RETURNS")), [row("RETURNS-2", true, 0), row("RETURNS-5", true, 0)], "RETURNS-5 moves up past RETURNS-4");
    // Off, `tree` is FILTER-13's: the holders are rows.
    let off = tree_answered(&store, &filter, None, Some("RETURNS"), None, None).unwrap().0;
    assert_eq!(off, [row("RETURNS-1", false, 2)]);
}

/// FILTER-24, the ticket's examples: `keep` draws a ticket the filter
/// excludes where a match would be — its nearest matches nested under it, a
/// shortcode hosted below it placed at it — and says the truth of it
/// (`matched: false`); a match, a root, a ticket that is none, and the tree
/// without `matches_only` answer as without it.
#[tokio::test]
async fn a_kept_ticket_is_drawn_where_a_match_would_be() {
    let store = Store::new();
    let mk = |epic: &str, kind: Kind, parent: Option<&str>| new_ticket(epic, kind, parent.map(String::from), Vec::new());
    for (epic, kind, parent) in [
        ("MENGER", Kind::Scope, None),            // MENGER-1, the root
        ("MENGER", Kind::Execution, Some("MENGER-1")), // MENGER-2, an area
        ("MENGER", Kind::Build, Some("MENGER-2")), // MENGER-3
        ("FILTER", Kind::Scope, Some("MENGER-2")), // FILTER-1, hosted at MENGER-2
        ("FILTER", Kind::Build, Some("FILTER-1")), // FILTER-2
        ("FILTER", Kind::Execution, Some("FILTER-1")), // FILTER-3
        ("FILTER", Kind::Build, Some("FILTER-3")), // FILTER-4
        ("RETURNS", Kind::Scope, None),           // RETURNS-1
        ("RETURNS", Kind::Build, Some("RETURNS-1")), // RETURNS-2
        ("RETURNS", Kind::Execution, Some("RETURNS-1")), // RETURNS-3
        ("RETURNS", Kind::Build, Some("RETURNS-3")), // RETURNS-4
    ] {
        store.create(SYS, mk(epic, kind, parent)).await.unwrap();
    }
    let filter = Filter::parse("kind build").unwrap();
    let ask = |keep: &str, parent: Option<&str>, epic: Option<&str>| -> Vec<(String, bool, u64)> {
        let got = tree_kept_in(&store, &filter, parent, epic, None, None, keep);
        assert_eq!(got, tree_kept_by_rule(&store, &Plain::of(&store), &filter, parent, epic, None, None, Some(keep)), "{parent:?} {epic:?} keeping {keep}");
        got.unwrap().0
    };
    let plain = |parent: Option<&str>, epic: Option<&str>| tree_answered_in(&store, &filter, parent, epic, None, None, true).unwrap().0;
    let row = |id: &str, matched: bool, below: u64| (id.to_string(), matched, below);

    // The open execution is drawn at the top of FILTER, beside FILTER-2; its
    // build is under it, no longer at the top.
    assert_eq!(plain(None, Some("FILTER")), [row("FILTER-2", true, 0), row("FILTER-4", true, 0)]);
    assert_eq!(ask("FILTER-3", None, Some("FILTER")), [row("FILTER-2", true, 0), row("FILTER-3", false, 1)]);
    assert_eq!(ask("filter-3", Some("FILTER-1"), None), [row("FILTER-2", true, 0), row("FILTER-3", false, 1)], "the root's address; any case");
    assert_eq!(ask("FILTER-3", Some("FILTER-3"), None), [row("FILTER-4", true, 0)]);
    assert_eq!(ask("RETURNS-3", None, Some("RETURNS")), [row("RETURNS-2", true, 0), row("RETURNS-3", false, 1)]);

    // An area kept: drawn at MENGER's top, and FILTER's row — hosted at it —
    // drawn under it, no longer moved up.
    assert_eq!(plain(None, Some("MENGER")), [row("MENGER-3", true, 0), row("FILTER-1", false, 2)]);
    assert_eq!(ask("MENGER-2", None, Some("MENGER")), [row("MENGER-2", false, 3)]);
    assert_eq!(ask("MENGER-2", Some("MENGER-2"), None), [row("MENGER-3", true, 0), row("FILTER-1", false, 2)]);
    // Below it, nothing moves: FILTER's own top is as it was.
    assert_eq!(ask("MENGER-2", None, Some("FILTER")), plain(None, Some("FILTER")));

    // Paged in its place.
    let (page, next, remaining) = tree_kept_in(&store, &filter, None, Some("RETURNS"), Some("RETURNS-2"), Some(1), "RETURNS-3").unwrap();
    assert_eq!((page, next, remaining), (vec![row("RETURNS-3", false, 1)], None, 0));

    // Nothing to keep: a match, a root, no ticket — and FILTER-13's tree.
    for keep in ["FILTER-2", "FILTER-1", "RETURNS-1", "NONE-9", ""] {
        for (parent, epic) in [(None, Some("FILTER")), (None, Some("MENGER")), (Some("FILTER-1"), None), (Some("MENGER-2"), None), (None, Some("RETURNS"))] {
            assert_eq!(ask(keep, parent, epic), plain(parent, epic), "{parent:?} {epic:?} keeping {keep:?}");
        }
    }
    let q = TreeQuery { parent: None, epic: Some("FILTER".into()), lens: Lens::of(&filter).unwrap(), matches_only: false, keep: Some("FILTER-3".into()) };
    let off = store.tree_page(SYS, &q, None, None).unwrap().nodes.into_iter().map(|n| (n.summary.id.into_string(), n.matched, n.below)).collect::<Vec<_>>();
    assert_eq!(Ok(off), tree_answered(&store, &filter, None, Some("FILTER"), None, None).map(|a| a.0), "keep is the nav's that shows only matches");
}

/// `tree`'s address: exactly one of `parent` and `epic`.
#[tokio::test]
async fn tree_answers_one_address() {
    let store = Store::new();
    store.create(SYS, new_ticket("TA", Kind::Scope, None, Vec::new())).await.unwrap();
    let lens = Lens::parse("").unwrap();
    for (parent, epic) in [(None, None), (Some("TA-1"), Some("TA"))] {
        let q = TreeQuery { parent: parent.map(String::from), epic: epic.map(String::from), lens: lens.clone(), matches_only: false, keep: None };
        let e = store.tree_page(SYS, &q, None, None).unwrap_err().to_string();
        assert!(e.contains("exactly one of them"), "{e}");
    }
    let q = TreeQuery { parent: None, epic: Some("ta".into()), lens, matches_only: false, keep: None };
    let page = store.tree_page(SYS, &q, None, None).unwrap();
    assert_eq!(page.nodes.iter().map(|n| (n.summary.id.as_str(), n.matched, n.below)).collect::<Vec<_>>(), [("TA-1", true, 0)]);
    assert_eq!((page.next, page.remaining), (None, 0));
}

// ---------------------------------------------------------------------------
// `watch { filter }` with `under`: what each write works out for the watch is
// exactly what a scan before and after it says entered and left — re-parents
// (which move no cell) included.

#[tokio::test]
async fn under_membership_is_the_scans_difference_through_re_parents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("under.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic_shaped(SYS, SeedSpec { count: 500, seed: 77, into_existing: false }, SeedShape::Nested).unwrap();
    let epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).take(3).collect();
    let mut rng = Rng(0x0D3_13);
    let names = filter_names(&store);
    let filters: Vec<Filter> = (0..8)
        .map(|i| {
            let x = names[rng.below(names.len())].to_ascii_uppercase();
            let f = random_filter(&mut rng, &names);
            match i % 3 {
                0 => Filter::parse(&format!("under {x}")).unwrap(),
                1 => f.and(&Filter::parse(&format!("under {x}")).unwrap()),
                _ => f.and(&Filter::parse(&format!("not under {x}")).unwrap()),
            }
        })
        .collect();
    let watchers: Vec<Watcher> = filters
        .iter()
        .enumerate()
        .map(|(i, f)| store.watcher_in(Lens::of(f).unwrap(), (i == 7).then_some(&ShortcodeId::from(epics[0].as_str()))))
        .collect();
    let mut rx = store.subscribe_filtered();
    let epic_refs: Vec<&str> = epics.iter().map(String::as_str).collect();
    let mut tally = Tally::default();
    let (mut moved, mut by_moves) = (0, 0);
    for step in 0..220 {
        let before = Plain::of(&store);
        // A third of the writes re-parent, inside a shortcode.
        let what = if rng.below(3) == 0 {
            let rows = &before.rows;
            let child = &rows[rng.below(rows.len())];
            let mine: Vec<&Row> = rows.iter().filter(|t| t.epic == child.epic).collect();
            let parent = mine[rng.below(mine.len())].id.clone();
            let _ = store.set_parent(SYS.moving(&child.id, &parent)).await;
            by_moves += 1;
            format!("set_parent {} under {parent}", child.id)
        } else {
            random_write(&store, &mut rng, &epic_refs, &mut tally).await
        };
        let after = Plain::of(&store);
        let mut writes: Vec<Arc<UnderMoves>> = Vec::new();
        while let Ok(change) = rx.try_recv() {
            if let Change::UnderMoved { moves } = change {
                writes.push(moves);
            }
        }
        assert!(writes.len() <= 1, "one UnderMoved a write");
        for (i, (f, w)) in filters.iter().zip(&watchers).enumerate() {
            let only = (i == 7).then_some(epics[0].as_str());
            let inside = |p: &Plain| -> BTreeMap<OrderKey, String> {
                p.rows
                    .iter()
                    .zip(&p.cells)
                    .filter(|(t, k)| only.is_none_or(|e| t.epic == e) && f.matches(&p.facts(t, **k)))
                    .map(|(t, _)| (OrderKey::of(t), t.id.to_string()))
                    .collect()
            };
            let (was, now) = (inside(&before), inside(&after));
            let mut want: Vec<(OrderKey, Membership)> = Vec::new();
            for (k, id) in &now {
                if !was.contains_key(k) {
                    want.push((k.clone(), Membership::Entered { id: id.clone(), shortcode: k.shortcode.clone(), seq: k.seq }));
                }
            }
            for (k, id) in &was {
                if !now.contains_key(k) {
                    want.push((k.clone(), Membership::Left { id: id.clone(), shortcode: k.shortcode.clone(), seq: k.seq }));
                }
            }
            want.sort_by(|a, b| a.0.cmp(&b.0));
            let got: Vec<Membership> = writes.iter().flat_map(|m| w.under_membership(m)).collect();
            if want.len() > cells::RECOUNT_OVER {
                let mut epics_moved: Vec<ShortcodeId> = want.iter().map(|(k, _)| k.shortcode.clone()).collect();
                epics_moved.dedup();
                let recounts: Vec<Membership> = epics_moved
                    .into_iter()
                    .map(|e| Membership::Recount { held: now.keys().filter(|k| k.shortcode == e).count() as u64, shortcode: e })
                    .collect();
                assert_eq!(got, recounts, "step {step}: {what}: `{}`", f.format());
            } else {
                let want: Vec<Membership> = want.into_iter().map(|(_, m)| m).collect();
                assert_eq!(got, want, "step {step}: {what}: `{}`", f.format());
            }
            moved += got.len();
        }
    }
    assert!(moved > 40 && by_moves > 40, "{moved} membership moves, {by_moves} re-parents");
}
