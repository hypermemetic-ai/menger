//! WAVE-4: `Store::waves` — each ticket's place in the order of work.
//!
//! Acceptance 1, a test per clause: a chain gives 1, 2, 3 and completing
//! (or superseding) its head moves the rest left; a diamond takes the max;
//! a dangling blocker puts the ticket and everything behind it in the
//! missing-blocker lane; a blocker in a shortcode not asked for still
//! places the ticket; docs never appear; `kinds` filters what is yielded,
//! not what is followed. Then what the rule leaves to the walk: the yield
//! order and the tally, `waiting_on`, a `Blocked` status and a `parent`
//! that order nothing, a hand-edited cycle that terminates, and a chain
//! longer than any call stack.

use super::waves::{Placed, Waves, DEFAULT_KINDS};
use super::*;
use crate::types::Placement;

fn new(epic: &str, kind: Kind, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: None,
        body: String::new(),
    }
}

/// A shortcode's root — every shortcode starts with a scope (MNG-12).
async fn root(store: &Store, epic: &str) -> String {
    store.create(new(epic, Kind::Scope, &[])).await.unwrap().id
}

async fn build(store: &Store, epic: &str, blocked_by: &[&str]) -> String {
    store.create(new(epic, Kind::Build, blocked_by)).await.unwrap().id
}

fn ask(store: &Store, epics: &[&str], kinds: &[Kind]) -> Waves {
    let epics: Vec<String> = epics.iter().map(|e| e.to_string()).collect();
    store.waves(&epics, kinds)
}

/// `id → placement` of the default (work) answer for `epics`.
fn placements(store: &Store, epics: &[&str]) -> BTreeMap<String, Placement> {
    ask(store, epics, &DEFAULT_KINDS)
        .placed
        .into_iter()
        .map(|p| (p.summary.id, p.placement))
        .collect()
}

fn wave(n: u32) -> Placement {
    Placement::Wave { n }
}

fn ids(placed: &[Placed]) -> Vec<&str> {
    placed.iter().map(|p| p.summary.id.as_str()).collect()
}

/// A → B → C (B waits on A, C on B) is waves 1, 2, 3; completing A moves B
/// to 1 and C to 2, and A is done; superseding has the same effect — and
/// putting A back (it is a free write) puts them back.
#[tokio::test]
async fn a_chain_is_one_two_three_and_finishing_its_head_moves_the_rest_left() {
    for finish in [Status::Complete, Status::Superseded] {
        let store = Store::new();
        root(&store, "CH").await;
        let a = build(&store, "CH", &[]).await;
        let b = build(&store, "CH", &[&a]).await;
        let c = build(&store, "CH", &[&b]).await;
        let at = |store: &Store| {
            let p = placements(store, &["CH"]);
            (p[&a], p[&b], p[&c])
        };
        assert_eq!(at(&store), (wave(1), wave(2), wave(3)), "the chain");

        store.set_status(&a, finish).await.unwrap();
        assert_eq!(at(&store), (Placement::Done, wave(1), wave(2)), "{finish}: A done, the rest one left");

        store.set_status(&b, finish).await.unwrap();
        assert_eq!(at(&store), (Placement::Done, Placement::Done, wave(1)), "{finish}");

        store.set_status(&a, Status::Ready).await.unwrap();
        store.set_status(&b, Status::Pending).await.unwrap();
        assert_eq!(at(&store), (wave(1), wave(2), wave(3)), "{finish}: undone, back where they were");
    }
}

/// The worker states and `Blocked` are not done: a blocker `In Review` still
/// holds what it blocks back (MNG-4's rule, SDLC-7).
#[tokio::test]
async fn only_complete_and_superseded_satisfy_a_blocker() {
    let store = Store::new();
    root(&store, "SAT").await;
    let a = build(&store, "SAT", &[]).await;
    let b = build(&store, "SAT", &[&a]).await;
    for s in [Status::Ready, Status::Coding, Status::InReview, Status::Blocked, Status::Idea, Status::Epic] {
        store.set_status(&a, s).await.unwrap();
        assert_eq!(placements(&store, &["SAT"])[&b], wave(2), "{s} does not satisfy");
    }
}

/// A diamond — D waits on B (wave 2) and C (wave 3) — takes the highest
/// of its blockers' waves: 4, not 3.
#[tokio::test]
async fn a_diamond_takes_the_highest_wave_behind_it() {
    let store = Store::new();
    root(&store, "DI").await;
    let a = build(&store, "DI", &[]).await; // 1
    let b = build(&store, "DI", &[&a]).await; // 2
    let x = build(&store, "DI", &[&b]).await; // 3
    let c = build(&store, "DI", &[&a, &x]).await; // 4
    let d = build(&store, "DI", &[&b, &c]).await; // 5
    let p = placements(&store, &["DI"]);
    assert_eq!([p[&a], p[&b], p[&x], p[&c], p[&d]], [wave(1), wave(2), wave(3), wave(4), wave(5)]);

    // Finishing the long side shortens the diamond to its other side.
    store.set_status(&x, Status::Complete).await.unwrap();
    let p = placements(&store, &["DI"]);
    assert_eq!([p[&c], p[&d]], [wave(2), wave(3)], "C now waits on A alone; D on B and C");
}

/// A blocker id that names no ticket puts its ticket in the missing-blocker
/// lane — and every ticket that waits on it, at any depth; a ticket beside
/// them is placed as ever. When the id is born, the lane empties into the
/// waves.
#[tokio::test]
async fn a_dangling_blocker_puts_the_ticket_and_its_dependents_in_the_missing_lane() {
    let store = Store::new();
    root(&store, "DA").await; // DA-1
    let lost = build(&store, "DA", &["DA-6"]).await; // DA-2 waits on DA-6, not born
    let behind = build(&store, "DA", &[&lost]).await; // DA-3
    let deeper = build(&store, "DA", &[&behind]).await; // DA-4
    let free = build(&store, "DA", &[]).await; // DA-5
    let p = placements(&store, &["DA"]);
    assert_eq!(p[&lost], Placement::MissingBlocker);
    assert_eq!(p[&behind], Placement::MissingBlocker);
    assert_eq!(p[&deeper], Placement::MissingBlocker);
    assert_eq!(p[&free], wave(1));
    let answer = ask(&store, &["DA"], &DEFAULT_KINDS);
    let of = |id: &str| answer.placed.iter().find(|p| p.summary.id == id).unwrap().waiting_on.clone();
    assert_eq!(of(&lost), ["DA-6"], "the missing id is what it waits on");
    assert_eq!(of(&behind), [lost.clone()]);

    // DA-6 is born: the forward reference resolves, and the lane empties.
    assert_eq!(build(&store, "DA", &[]).await, "DA-6");
    let p = placements(&store, &["DA"]);
    assert_eq!((p[&lost], p[&behind], p[&deeper], p["DA-6"]), (wave(2), wave(3), wave(4), wave(1)));
}

/// A missing blocker anywhere behind a ticket wins over any wave: a ticket
/// waiting on a wave-3 ticket and on a missing-lane one is missing.
#[tokio::test]
async fn missing_wins_over_a_wave_beside_it() {
    let store = Store::new();
    root(&store, "MW").await;
    let a = build(&store, "MW", &[]).await;
    let b = build(&store, "MW", &[&a]).await;
    let lost = build(&store, "MW", &["NOWHERE-1"]).await;
    let t = build(&store, "MW", &[&b, &lost]).await;
    assert_eq!(placements(&store, &["MW"])[&t], Placement::MissingBlocker);
}

/// Blockers are followed into every shortcode: a ticket of `HERE` that
/// waits on a wave-2 ticket of `THERE` is wave 3 when only `HERE` is asked
/// — and nothing of `THERE` is yielded. Its waiting-on names the ticket
/// elsewhere (the board's off-board chip).
#[tokio::test]
async fn a_blocker_in_another_shortcode_places_the_ticket_though_only_its_own_is_asked() {
    let store = Store::new();
    root(&store, "THERE").await;
    let t1 = build(&store, "THERE", &[]).await;
    let t2 = build(&store, "THERE", &[&t1]).await;
    root(&store, "HERE").await;
    let h = build(&store, "HERE", &[&t2]).await;

    let answer = ask(&store, &["here"], &DEFAULT_KINDS);
    assert_eq!(ids(&answer.placed), [h.as_str()], "only HERE's work is yielded; case-insensitive");
    assert_eq!(answer.placed[0].placement, wave(3));
    assert_eq!(answer.placed[0].waiting_on, [t2.clone()]);
    assert_eq!(answer.waves, [0, 0, 1], "waves 1 and 2 hold nothing of HERE's");

    store.set_status(&t1, Status::Complete).await.unwrap();
    assert_eq!(placements(&store, &["HERE"])[&h], wave(2), "a change elsewhere moves it");
}

/// A doc is never placed — not by default, and not when `kinds` names it —
/// and is not counted.
#[tokio::test]
async fn docs_never_appear() {
    let store = Store::new();
    root(&store, "DOCS").await;
    store.create(new("DOCS", Kind::Doc, &[])).await.unwrap();
    let w = build(&store, "DOCS", &[]).await;
    let manual = store.create(new("MAN", Kind::Doc, &[])).await.unwrap().id; // a doc collection
    store.create(new("MAN", Kind::Doc, &[])).await.unwrap();

    assert_eq!(ids(&ask(&store, &["DOCS", "MAN"], &DEFAULT_KINDS).placed), [w.as_str()]);
    let every = ask(&store, &["DOCS", "MAN"], &Kind::ALL);
    assert_eq!(ids(&every.placed), ["DOCS-1", w.as_str()], "the scope root and the build; no doc");
    assert!(!ids(&every.placed).contains(&manual.as_str()));
    assert_eq!((every.waves.clone(), every.missing, every.done), (vec![2], 0, 0));
    assert!(ask(&store, &["MAN"], &[Kind::Doc]).placed.is_empty(), "asking for docs yields none");
}

/// `kinds` filters what is yielded, never what is followed: a build waiting
/// on an unfinished scope is wave 2 whether or not scopes are asked for;
/// the scope itself appears only when they are.
#[tokio::test]
async fn kinds_filter_what_is_yielded_not_what_is_followed() {
    let store = Store::new();
    let r = root(&store, "KI").await; // KI-1, a scope, wave 1
    let s = store.create(new("KI", Kind::Spike, &[&r])).await.unwrap().id;
    let g = store.create(new("KI", Kind::Gap, &[&s])).await.unwrap().id;
    let e = store.create(new("KI", Kind::Execution, &[&g])).await.unwrap().id;
    let b = build(&store, "KI", &[&e]).await;

    let work = ask(&store, &["KI"], &DEFAULT_KINDS);
    assert_eq!(ids(&work.placed), [s.as_str(), g.as_str(), b.as_str()], "build, spike, gap by default");
    let p: BTreeMap<_, _> = work.placed.iter().map(|p| (p.summary.id.clone(), p.placement)).collect();
    assert_eq!((p[&s], p[&g], p[&b]), (wave(2), wave(3), wave(5)), "the scope and the execution still count");
    assert_eq!(work.waves, [0, 1, 1, 0, 1]);

    let only = ask(&store, &["KI"], &[Kind::Scope, Kind::Execution]);
    assert_eq!(ids(&only.placed), [r.as_str(), e.as_str()]);
    assert_eq!(only.placed[1].placement, wave(4));
    assert!(ask(&store, &["KI"], &[]).placed.is_empty(), "no kinds, nothing yielded");
}

/// The yield order — waves ascending; inside one, Coding, In Review, Ready,
/// Pending, Blocked, Idea, Epic, then the store's order across shortcodes;
/// then the missing lane; then the done, each in the store's order — and
/// the tally that closes it.
#[tokio::test]
async fn the_yield_order_and_the_tally() {
    let store = Store::new();
    root(&store, "AA").await;
    root(&store, "BB").await;
    // Wave 1, one of each unfinished status, filed against the order they
    // must come out in, across two shortcodes.
    let mut wave1 = Vec::new();
    for (epic, s) in [
        ("BB", Status::Epic),
        ("AA", Status::Idea),
        ("BB", Status::Blocked),
        ("AA", Status::Pending),
        ("BB", Status::Pending),
        ("AA", Status::Ready),
        ("BB", Status::InReview),
        ("AA", Status::Coding),
        ("BB", Status::Coding),
    ] {
        let id = build(&store, epic, &[]).await;
        store.set_status(&id, s).await.unwrap();
        wave1.push((s, id));
    }
    let head = wave1.iter().find(|(s, _)| *s == Status::Epic).unwrap().1.clone();
    let w2_ready = build(&store, "BB", &[&head]).await;
    store.set_status(&w2_ready, Status::Ready).await.unwrap();
    let w2_coding = build(&store, "BB", &[&head]).await;
    store.set_status(&w2_coding, Status::Coding).await.unwrap();
    let missing_b = build(&store, "BB", &["GONE-1"]).await;
    let missing_a = build(&store, "AA", &["GONE-2"]).await;
    let done_b = build(&store, "BB", &[]).await;
    store.set_status(&done_b, Status::Complete).await.unwrap();
    let done_a = build(&store, "AA", &[&missing_a]).await; // done though its blocker is missing
    store.set_status(&done_a, Status::Superseded).await.unwrap();

    let answer = ask(&store, &["BB", "AA"], &DEFAULT_KINDS);
    let got: Vec<&str> = ids(&answer.placed);
    let id = |s: Status, epic: &str| wave1.iter().find(|(st, i)| *st == s && i.starts_with(epic)).unwrap().1.as_str();
    let want = vec![
        id(Status::Coding, "AA"),
        id(Status::Coding, "BB"),
        id(Status::InReview, "BB"),
        id(Status::Ready, "AA"),
        id(Status::Pending, "AA"),
        id(Status::Pending, "BB"),
        id(Status::Blocked, "BB"),
        id(Status::Idea, "AA"),
        id(Status::Epic, "BB"),
        w2_coding.as_str(),
        w2_ready.as_str(),
        missing_a.as_str(),
        missing_b.as_str(),
        done_a.as_str(),
        done_b.as_str(),
    ];
    assert_eq!(got, want);
    assert_eq!((answer.waves.clone(), answer.missing, answer.done), (vec![9, 2], 2, 2));
    let done: Vec<&Placed> = answer.placed.iter().filter(|p| p.placement == Placement::Done).collect();
    assert!(done.iter().all(|p| p.waiting_on.is_empty()), "a done ticket waits on nothing");

    let none = ask(&store, &["NOPE"], &DEFAULT_KINDS);
    assert_eq!(none, Waves::default(), "a shortcode with no tickets contributes nothing");
    let twice = ask(&store, &["aa", "AA", "NOPE"], &DEFAULT_KINDS);
    assert_eq!(ids(&twice.placed), ids(&ask(&store, &["AA"], &DEFAULT_KINDS).placed), "asked twice, yielded once");
}

/// `waiting_on` is the unsatisfied direct blockers — missing ids included,
/// satisfied ones left out — in `blocked_by` order, each once.
#[tokio::test]
async fn waiting_on_is_the_unsatisfied_direct_blockers_in_order_each_once() {
    let store = Store::new();
    root(&store, "WO").await;
    let a = build(&store, "WO", &[]).await; // WO-2
    let b = build(&store, "WO", &[]).await; // WO-3
    let c = build(&store, "WO", &[]).await; // WO-4
    store.set_status(&b, Status::Complete).await.unwrap();
    let t = build(&store, "WO", &[&c, "WO-99", &b, &a, &c]).await;
    let answer = ask(&store, &["WO"], &DEFAULT_KINDS);
    let placed = answer.placed.iter().find(|p| p.summary.id == t).unwrap();
    assert_eq!(placed.waiting_on, [c.clone(), "WO-99".to_string(), a.clone()]);
    assert_eq!(placed.placement, Placement::MissingBlocker);
    let free = answer.placed.iter().find(|p| p.summary.id == a).unwrap();
    assert!(free.waiting_on.is_empty());
}

/// Only `blocked_by` orders the waves: a ticket whose status is `Blocked`
/// with no blocker is wave 1 (the status is a marker on its card), and a
/// parent — however unfinished — holds no child back.
#[tokio::test]
async fn a_blocked_status_and_a_parent_order_nothing() {
    let store = Store::new();
    root(&store, "PO").await;
    let marked = build(&store, "PO", &[]).await;
    store.set_status(&marked, Status::Blocked).await.unwrap();
    let parent = build(&store, "PO", &[]).await;
    let child = store
        .create(NewTicket { parent: Some(parent.clone()), ..new("PO", Kind::Build, &[]) })
        .await
        .unwrap()
        .id;
    let p = placements(&store, &["PO"]);
    assert_eq!((p[&marked], p[&parent], p[&child]), (wave(1), wave(1), wave(1)));
}

/// A `blocked_by` cycle — `create` refuses one, so made here by hand in the
/// cache — terminates, and places every ticket on it, and every ticket
/// behind it, in the missing lane: no status change can end that wait.
/// What does not wait on the cycle is placed as ever.
#[tokio::test]
async fn a_hand_edited_cycle_terminates_in_the_missing_lane() {
    let store = Store::new();
    root(&store, "CY").await;
    let a = build(&store, "CY", &[]).await; // CY-2
    let b = build(&store, "CY", &[&a]).await; // CY-3
    let c = build(&store, "CY", &[&b]).await; // CY-4, behind the cycle
    let free = build(&store, "CY", &[]).await; // CY-5
    let mine = build(&store, "CY", &[&free]).await; // CY-6
    let selfish = build(&store, "CY", &[]).await; // CY-7
    {
        let mut cache = store.tickets.write().unwrap();
        for (id, blocked_by) in [(&a, vec![b.clone()]), (&selfish, vec![selfish.clone()])] {
            let key = cache.at[id.as_str()].clone();
            cache.order.get_mut(&key).unwrap().blocked_by = blocked_by;
        }
        // WAVE-6: a hand edit reaches a store through its file, and the
        // store places every ticket when it opens it — as here.
        cache.place_all();
    }
    let p = placements(&store, &["CY"]);
    assert_eq!(p[&a], Placement::MissingBlocker);
    assert_eq!(p[&b], Placement::MissingBlocker);
    assert_eq!(p[&c], Placement::MissingBlocker);
    assert_eq!(p[&selfish], Placement::MissingBlocker, "a ticket that names itself");
    assert_eq!((p[&free], p[&mine]), (wave(1), wave(2)));

    // Entered from any ticket on or behind it, with nothing placed yet, the
    // answer is the same.
    let cache = store.tickets.read().unwrap();
    for id in [&a, &b, &c, &selfish] {
        let mut memo = HashMap::new();
        let placement = waves::place(cache.rows(), cache.get(id).unwrap(), &mut memo);
        assert_eq!(placement, Placement::MissingBlocker, "entered at {id}");
    }
}

/// The walk is iterative: a chain of 200,000 tickets — far past what a
/// recursive walk's stack would hold — places its tail at wave 200,000,
/// built as rows straight into a cache's halves and asked of `waves` itself.
#[test]
fn a_chain_longer_than_any_call_stack_is_placed() {
    const N: u32 = 200_000;
    let row = |seq: u32| Row {
        labels: Vec::new(),
        id: format!("LONG-{seq}"),
        epic: "LONG".into(),
        seq,
        title: String::new(),
        status: Status::Pending,
        kind: Kind::Build,
        severity: None,
        parent: None,
        child_count: 0,
        blocked_by: if seq > 1 { vec![format!("LONG-{}", seq - 1)] } else { vec![] },
        unlocks: vec![],
        commits: vec![],
        path: None,
        placement: None,
        created_unix: 0,
        updated_unix: 0,
    };
    let loaded: HashMap<String, Row> = (1..=N).map(|s| (format!("LONG-{s}"), row(s))).collect();
    let (order, at, _) = ordered(loaded);
    let children = children::Children::build(order.values());
    let rows = Rows { order: &order, at: &at, children: &children, patch: None };
    let epics: std::collections::BTreeSet<String> = ["LONG".to_string()].into();

    // Run where the stack is small, so a recursive walk could not pass.
    let answer = std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn_scoped(s, || waves::waves(rows, &epics, &DEFAULT_KINDS))
            .expect("spawn")
            .join()
            .expect("the walk does not overflow")
    });
    assert_eq!(answer.placed.len(), N as usize);
    assert_eq!(answer.placed.last().unwrap().placement, wave(N));
    assert_eq!(answer.waves.len(), N as usize);
    assert!(answer.waves.iter().all(|n| *n == 1));
}
