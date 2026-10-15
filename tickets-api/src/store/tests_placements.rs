//! WAVE-6: the placements kept — on every row, equal at every moment to
//! WAVE-4's rule over the whole graph ([`waves::place`]), moved by the write
//! that moves them and said once per write in `placements_changed`.
//!
//! Acceptance 1 is the property test: random sequences of births (forward
//! references, cross-shortcode blockers, docs) and status changes (into and
//! out of `Complete` / `Superseded`), each followed by the kept placements
//! held to the rule, a mirror kept from `watch` alone held to the kept ones,
//! and `waves` held to its own answer computed from nothing kept. Acceptance
//! 2 is the two-blockers-back case. Then what the pass promises beyond its
//! answers: it stops where nothing moves, a birth resolves the tickets a
//! forward reference held missing, a reopening moves them back, a hand-edited
//! cycle gives way to the whole store, the placements come back after a
//! restart, and `watch { epic }` keeps the changes in its shortcode.

use super::waves::{Waves, DEFAULT_KINDS};
use super::*;
use super::tests::Changes;
use crate::types::{Placement, PlacementChange};
use std::collections::BTreeSet;

/// Every kept placement is the rule's: WAVE-4's walk, from nothing kept,
/// over the rows as they stand — `None` exactly for a doc.
#[track_caller]
pub(super) fn assert_kept_placements_are_the_rule(store: &Store, after: &str) {
    let cache = store.tickets.read().unwrap();
    let mut memo = HashMap::new();
    for t in cache.order.values() {
        let want = t.kind.has_lifecycle().then(|| waves::place(cache.rows(), t, &mut memo));
        assert_eq!(t.placement, want, "{}'s kept placement, after {after}", t.id);
    }
}

fn new(epic: &str, kind: Kind, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| (*b).into()).collect(),
        parent: None,
        path: None,
        body: String::new(),
    }
}

async fn build(store: &Store, epic: &str, blocked_by: &[&str]) -> String {
    String::from(store.create(SYS, new(epic, Kind::Build, blocked_by)).await.unwrap().id)
}

fn wave(n: u32) -> Placement {
    Placement::Wave { n }
}

fn kept(store: &Store, id: &str) -> Option<Placement> {
    store.row(id).unwrap().0.placement
}

/// Every change `watch` was told of so far.
fn drain(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    let mut out = Vec::new();
    while let Ok(c) = rx.try_change() {
        out.push(c);
    }
    out
}

/// The `placements_changed` among `changes` — asserting there is at most
/// one, and that it is the last.
#[track_caller]
fn placed(changes: &[Change]) -> Option<Vec<(String, Placement)>> {
    let at: Vec<usize> = changes
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, Change::PlacementsChanged { .. }))
        .map(|(i, _)| i)
        .collect();
    assert!(at.len() <= 1, "one placements_changed per write: {changes:?}");
    let i = *at.first()?;
    assert_eq!(i, changes.len() - 1, "after the write's other events: {changes:?}");
    let Change::PlacementsChanged { changes } = &changes[i] else { unreachable!() };
    Some(changes.iter().map(|(_, c)| (c.id.clone(), c.placement)).collect())
}

/// `waves` as WAVE-4 answered it — every placement worked out by the rule,
/// nothing kept — for the property test's "output unchanged".
fn waves_from_nothing_kept(store: &Store, epics: &BTreeSet<String>, kinds: &[Kind]) -> Waves {
    let cache = store.tickets.read().unwrap();
    let order: BTreeMap<OrderKey, Row> =
        cache.order.iter().map(|(k, t)| (k.clone(), Row { placement: None, ..t.clone() })).collect();
    let rows = Rows {
        order: &order,
        at: &cache.at,
        children: &cache.children,
        patch: None,
    };
    let epics: std::collections::BTreeSet<ShortcodeId> = epics.iter().map(ShortcodeId::from).collect();
    let mut w = waves::waves(rows, &epics, kinds);
    // The summaries differ by the kept placement alone; compare the rest.
    for p in &mut w.placed {
        p.summary.placement = None;
    }
    w
}

/// SplitMix64, as the seed generator has it: reproducible from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// Acceptance 1: after any random sequence of births — forward references,
/// blockers in other shortcodes, ids that never come, docs — and status
/// changes into and out of `Complete` / `Superseded`, every kept placement
/// is WAVE-4's; a client that holds only what `watch` said (each `created`,
/// each `status_changed`, each `placements_changed`) holds the same; and
/// `waves` answers what it answered when it kept nothing.
#[tokio::test]
async fn kept_placements_are_the_rule_after_any_sequence_of_births_and_status_changes() {
    const EPICS: [&str; 3] = ["PA", "PB", "PC"];
    const STATUSES: [Status; 10] = Status::LIFECYCLE;
    for seed in 0..120u64 {
        let store = Store::new();
        let mut rng = Rng(seed);
        let mut mirror: BTreeMap<String, Option<Placement>> = BTreeMap::new();
        let mut ids: Vec<String> = Vec::new();
        for e in EPICS {
            let t = store.create(SYS, new(e, Kind::Scope, &[])).await.unwrap();
            mirror.insert(t.id.to_string(), t.placement);
            ids.push(String::from(t.id));
        }
        let mut rx = store.subscribe();
        let mut moved_total = 0usize;
        for step in 0..60 {
            let op = rng.below(100);
            let what: String;
            if op < 55 {
                // A birth, with up to three blockers: tickets that exist, ids
                // a later birth will take, an id nothing ever takes.
                let epic = EPICS[rng.below(EPICS.len())];
                let kind = match rng.below(10) {
                    0 => Kind::Doc,
                    1 => Kind::Scope,
                    2 => Kind::Spike,
                    3 => Kind::Gap,
                    4 => Kind::Execution,
                    _ => Kind::Build,
                };
                let mut blocked_by: Vec<String> = Vec::new();
                if kind != Kind::Doc {
                    for _ in 0..rng.below(4) {
                        let b = match rng.below(10) {
                            0..=5 => ids[rng.below(ids.len())].clone(),
                            6..=8 => {
                                let e = EPICS[rng.below(EPICS.len())];
                                let last = store.extents_text().into_iter().find(|(x, _)| x == e).map_or(0, |(_, s)| s);
                                format!("{e}-{}", last + 1 + rng.below(4) as u32)
                            }
                            _ => "GHOST-1".to_string(),
                        };
                        if !blocked_by.contains(&b) {
                            blocked_by.push(b);
                        }
                    }
                }
                let refs: Vec<&str> = blocked_by.iter().map(String::as_str).collect();
                what = format!("create {epic} {kind:?} blocked by {refs:?}");
                // A cycle, a doc named, a doc born as an awaited id: refused,
                // as `create` refuses them — nothing changed.
                match store.create(SYS, new(epic, kind, &refs)).await {
                    Ok(t) => ids.push(String::from(t.id)),
                    Err(e) => {
                        let heard = drain(&mut rx);
                        assert!(heard.is_empty(), "a refusal says nothing: {what}: {e}: {heard:?}");
                        continue;
                    }
                }
            } else {
                let id = ids[rng.below(ids.len())].clone();
                let status = if rng.chance(60) {
                    [Status::Complete, Status::Superseded, Status::Pending, Status::Ready][rng.below(4)]
                } else {
                    STATUSES[rng.below(STATUSES.len())]
                };
                what = format!("set_status {id} {status}");
                if let Err(e) = store.set_status(SYS.permit(&id), status).await {
                    // A doc: no lifecycle.
                    let heard = drain(&mut rx);
                    assert!(heard.is_empty(), "a refusal says nothing: {what}: {e}: {heard:?}");
                    continue;
                }
            }
            let after = format!("seed {seed} step {step}: {what}");
            let heard = drain(&mut rx);
            for c in &heard {
                match c {
                    Change::Created { ticket } | Change::StatusChanged { ticket, .. } | Change::UnlocksChanged { ticket, .. } => {
                        mirror.insert(ticket.id.to_string(), ticket.placement);
                    }
                    _ => {}
                }
            }
            if let Some(changes) = placed(&heard) {
                assert!(!changes.is_empty(), "a placements_changed moves something: {after}");
                moved_total += changes.len();
                for (id, p) in changes {
                    mirror.insert(id, Some(p));
                }
            }
            assert_kept_placements_are_the_rule(&store, &after);
            let kept_all: BTreeMap<String, Option<Placement>> = {
                let cache = store.tickets.read().unwrap();
                cache.order.values().map(|t| (t.id.to_string(), t.placement)).collect()
            };
            for (id, p) in &mirror {
                assert_eq!(kept_all.get(id), Some(p), "watch's {id}, after {after}");
            }
            let epics: BTreeSet<String> = EPICS.iter().map(|e| e.to_string()).collect();
            for kinds in [&DEFAULT_KINDS[..], &Kind::ALL[..]] {
                let mut answer = store.waves(SYS, &epics.iter().cloned().collect::<Vec<_>>(), kinds);
                for p in &mut answer.placed {
                    p.summary.placement = None;
                }
                assert_eq!(answer, waves_from_nothing_kept(&store, &epics, kinds), "waves, after {after}");
            }
        }
        assert!(moved_total > 0 || seed > 3, "seed {seed} moved nothing at all");
    }
    assert_eq!(
        placements::WHOLE.with(|w| w.get()),
        0,
        "nothing the store makes sends the walk to the whole store"
    );
}

/// Acceptance 2: C waits on X, X waits on Y. Completing Y moves X and C —
/// two blockers back from C — in the one `placements_changed` the write
/// makes, after its `status_changed`, which carries Y's own; and reopening Y
/// moves them back.
#[tokio::test]
async fn completing_a_blocker_two_back_moves_the_card_in_one_placements_changed() {
    let store = Store::new();
    store.create(SYS, new("TB", Kind::Scope, &[])).await.unwrap();
    let y = build(&store, "TB", &[]).await;
    let x = build(&store, "TB", &[&y]).await;
    let c = build(&store, "TB", &[&x]).await;
    assert_eq!((kept(&store, &y), kept(&store, &x), kept(&store, &c)), (Some(wave(1)), Some(wave(2)), Some(wave(3))));

    let mut rx = store.subscribe();
    store.set_status(SYS.permit(&y), Status::Complete).await.unwrap();
    let heard = drain(&mut rx);
    match &heard[0] {
        Change::StatusChanged { ticket, .. } => assert_eq!(ticket.placement, Some(Placement::Done)),
        other => panic!("expected StatusChanged first, got {other:?}"),
    }
    assert_eq!(
        placed(&heard).expect("one placements_changed"),
        vec![(y.clone(), Placement::Done), (x.clone(), wave(1)), (c.clone(), wave(2))]
    );
    assert_eq!((kept(&store, &x), kept(&store, &c)), (Some(wave(1)), Some(wave(2))));

    store.set_status(SYS.permit(&y), Status::Ready).await.unwrap();
    assert_eq!(
        placed(&drain(&mut rx)).unwrap(),
        vec![(y.clone(), wave(1)), (x.clone(), wave(2)), (c.clone(), wave(3))],
        "a reopening moves them back"
    );
    // A status change that stays on one side of the line moves nothing.
    store.set_status(SYS.permit(&y), Status::Coding).await.unwrap();
    assert_eq!(placed(&drain(&mut rx)), None);
    assert_kept_placements_are_the_rule(&store, "the two-back case");
}

/// The pass stops where nothing moves: completing A, which D waits on beside
/// a longer wait, moves A alone — D is placed again and stays, and the fifty
/// tickets behind D are not looked at.
#[tokio::test]
async fn the_pass_stops_where_nothing_moves() {
    let store = Store::new();
    store.create(SYS, new("ST", Kind::Scope, &[])).await.unwrap();
    let a = build(&store, "ST", &[]).await;
    let h1 = build(&store, "ST", &[]).await;
    let h2 = build(&store, "ST", &[&h1]).await;
    let h3 = build(&store, "ST", &[&h2]).await;
    let d = build(&store, "ST", &[&a, &h3]).await;
    let mut behind = d.clone();
    for _ in 0..50 {
        behind = build(&store, "ST", &[&behind]).await;
    }
    assert_eq!((kept(&store, &d), kept(&store, &behind)), (Some(wave(4)), Some(wave(54))));

    let (mut row, _) = store.row(&a).unwrap();
    row.status = Status::Complete;
    let replaced = {
        let cache = store.tickets.read().unwrap();
        cache.status_change(&row, Status::Pending).moved.placed.expect("a crossing re-places")
    };
    assert_eq!(replaced.placement, Some(Placement::Done));
    assert_eq!(replaced.placed, 2, "A and D, and nothing behind D");
    assert!(!replaced.whole);
    assert_eq!(replaced.shifts.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [a.as_str()]);

    let mut rx = store.subscribe();
    store.set_status(SYS.permit(&a), Status::Complete).await.unwrap();
    assert_eq!(placed(&drain(&mut rx)).unwrap(), vec![(a.clone(), Placement::Done)]);
    assert_eq!(kept(&store, &behind), Some(wave(54)));
    // The long wait ends: everything behind it moves, each once.
    store.set_status(SYS.permit(&h1), Status::Complete).await.unwrap();
    let moved = placed(&drain(&mut rx)).unwrap();
    assert_eq!(moved.len(), 54, "H1, H2, H3, D and the fifty behind D");
    assert_eq!(moved.last(), Some(&(behind.clone(), wave(53))), "D in wave 3 now, fifty behind it");
    assert_kept_placements_are_the_rule(&store, "the long wait");
}

/// A birth places the newborn — its placement on its `created` — and the
/// forward reference it resolves moves the tickets that named it out of the
/// missing lane, and theirs, in one `placements_changed` that does not name
/// the newborn.
#[tokio::test]
async fn a_birth_resolves_what_a_forward_reference_held_missing() {
    let store = Store::new();
    store.create(SYS, new("FW", Kind::Scope, &[])).await.unwrap();
    let t = build(&store, "FW", &["FW-4"]).await; // FW-2
    let u = build(&store, "FW", &[&t]).await; // FW-3
    assert_eq!((kept(&store, &t), kept(&store, &u)), (Some(Placement::MissingBlocker), Some(Placement::MissingBlocker)));

    let mut rx = store.subscribe();
    let born = store.create(SYS, new("FW", Kind::Build, &[])).await.unwrap();
    assert_eq!(born.id, "FW-4");
    assert_eq!(born.placement, Some(wave(1)), "born placed");
    let heard = drain(&mut rx);
    match &heard[0] {
        Change::Created { ticket } => assert_eq!(ticket.placement, Some(wave(1))),
        other => panic!("expected Created, got {other:?}"),
    }
    assert_eq!(placed(&heard).unwrap(), vec![(t.clone(), wave(2)), (u.clone(), wave(3))]);

    // A birth that resolves nothing moves nothing — its own placement is on
    // its `created`.
    build(&store, "FW", &[&u]).await;
    assert_eq!(placed(&drain(&mut rx)), None);
    // A doc is placed nowhere.
    let doc = store.create(SYS, new("FW", Kind::Doc, &[])).await.unwrap();
    assert_eq!(doc.placement, None);
    assert_eq!(placed(&drain(&mut rx)), None);
    assert_kept_placements_are_the_rule(&store, "the forward reference");
}

/// What only a hand edit makes: a `blocked_by` cycle, kept open by a
/// completed member, closed by reopening it. The walk notices and gives way
/// to the whole store, which places every ticket on and behind the cycle
/// missing, as the rule does.
#[tokio::test]
async fn reopening_a_hand_edited_cycle_gives_way_to_the_whole_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cycle.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, new("CY", Kind::Scope, &[])).await.unwrap();
        let a = build(&store, "CY", &[]).await; // CY-2
        build(&store, "CY", &[&a]).await; // CY-3, waits on CY-2
        build(&store, "CY", &["CY-3"]).await; // CY-4, behind them
        store.set_status(SYS.permit(&a), Status::Complete).await.unwrap();
    }
    {
        // By hand, both ends mirrored: CY-2 waits on CY-3 too.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "UPDATE tickets SET blocked_by = '[\"CY-3\"]' WHERE id = 'CY-2';
             UPDATE tickets SET unlocks = '[\"CY-4\",\"CY-2\"]' WHERE id = 'CY-3';",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_kept_placements_are_the_rule(&store, "the open");
    assert_eq!((kept(&store, "CY-3"), kept(&store, "CY-4")), (Some(wave(1)), Some(wave(2))), "CY-2 done: no cycle");

    let (mut row, _) = store.row("CY-2").unwrap();
    row.status = Status::Pending;
    let replaced = {
        let cache = store.tickets.read().unwrap();
        cache.status_change(&row, Status::Complete).moved.placed.unwrap()
    };
    assert!(replaced.whole, "the walk gave way");
    let mut rx = store.subscribe();
    store.set_status(SYS.permit("CY-2"), Status::Pending).await.unwrap();
    assert_eq!(
        placed(&drain(&mut rx)).unwrap(),
        vec![
            ("CY-2".to_string(), Placement::MissingBlocker),
            ("CY-3".to_string(), Placement::MissingBlocker),
            ("CY-4".to_string(), Placement::MissingBlocker),
        ]
    );
    assert_kept_placements_are_the_rule(&store, "the cycle closed");
}

/// Nothing stores a placement, so nothing is stale after a restart: the
/// store places every ticket from the rows when it opens the file.
#[tokio::test]
async fn placements_are_placed_again_at_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reopen.db");
    let before: BTreeMap<String, Option<Placement>> = {
        let store = Store::open(&path).unwrap();
        store.create(SYS, new("RO", Kind::Scope, &[])).await.unwrap();
        let a = build(&store, "RO", &[]).await;
        let b = build(&store, "RO", &[&a, "RO-9"]).await;
        build(&store, "RO", &[&b]).await;
        let c = build(&store, "RO", &[&a]).await;
        store.create(SYS, new("RO", Kind::Doc, &[])).await.unwrap();
        store.set_status(SYS.permit(&a), Status::Superseded).await.unwrap();
        build(&store, "RO", &[&c]).await;
        let cache = store.tickets.read().unwrap();
        cache.order.values().map(|t| (t.id.to_string(), t.placement)).collect()
    };
    let store = Store::open(&path).unwrap();
    let after: BTreeMap<String, Option<Placement>> = {
        let cache = store.tickets.read().unwrap();
        cache.order.values().map(|t| (t.id.to_string(), t.placement)).collect()
    };
    assert_eq!(after, before);
    assert_eq!(after["RO-6"], None, "a doc");
    assert_eq!(after["RO-3"], Some(Placement::MissingBlocker));
    assert_kept_placements_are_the_rule(&store, "a reopen");
    // `get` and a page carry it.
    assert_eq!(store.get(SYS, "RO-5").await.unwrap().placement, Some(wave(1)));
    let page = store.page(SYS, PageQuery { epic: Some("RO"), ..Default::default() }).unwrap();
    assert_eq!(page.tickets[6].placement, Some(wave(2)), "RO-7");
}

/// `watch { epic }` keeps the changes in its shortcode — and the event only
/// when one is left; with no epic, all of it.
#[tokio::test]
async fn watch_by_epic_keeps_the_changes_in_its_shortcode() {
    let store = Store::new();
    store.create(SYS, new("UP", Kind::Scope, &[])).await.unwrap();
    store.create(SYS, new("DN", Kind::Scope, &[])).await.unwrap();
    store.create(SYS, new("NO", Kind::Scope, &[])).await.unwrap();
    let up = build(&store, "UP", &[]).await;
    let near = build(&store, "UP", &[&up]).await;
    let far = build(&store, "DN", &[&near]).await;
    let mut rx = store.subscribe();
    store.set_status(SYS.permit(&up), Status::Complete).await.unwrap();
    let change = drain(&mut rx).pop().unwrap();
    let ids = |c: Option<Change>| match c {
        Some(Change::PlacementsChanged { changes }) => changes.into_iter().map(|(_, c)| c.id).collect::<Vec<_>>(),
        Some(other) => panic!("expected PlacementsChanged, got {other:?}"),
        None => Vec::new(),
    };
    assert_eq!(ids(change.clone().in_epic(None)), [up.clone(), near.clone(), far.clone()]);
    assert_eq!(ids(change.clone().in_epic(Some(&ShortcodeId::from("UP")))), [up, near]);
    assert_eq!(ids(change.clone().in_epic(Some(&ShortcodeId::from("DN")))), [far]);
    assert!(change.in_epic(Some(&ShortcodeId::from("NO"))).is_none(), "nothing of it in NO: no event");
    // The wire's form: the id and the placement.
    let c = PlacementChange { id: "DN-2".into(), placement: wave(2) };
    assert_eq!(
        serde_json::to_value(crate::types::TicketsEvent::PlacementsChanged { changes: vec![c] }).unwrap(),
        serde_json::json!({"type": "placements_changed", "changes": [{"id": "DN-2", "placement": {"type": "wave", "n": 2}}]})
    );
}
