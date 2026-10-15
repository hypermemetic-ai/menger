//! MENGER-31: `Store::waves_to` — the wave board whose final state is one
//! ticket — and `Store::waves_under`, a ticket's subtree's board.
//!
//! The set and the order on fixture graphs: a chain, a diamond, done
//! blockers (in, done, not followed past), a scope's open descendants and
//! what they wait on, a hand-edited cycle (named, terminating), `kinds`
//! filtering the yield and not the walk; and a subtree across a hosted
//! shortcode.

use super::waves::WavesTo;
use super::*;
use crate::types::Placement;

fn new(epic: &str, kind: Kind, blocked_by: &[&str], parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(str::to_string),
        body: String::new(),
    }
}

async fn make(store: &Store, epic: &str, kind: Kind, blocked_by: &[&str], parent: Option<&str>) -> String {
    String::from(store.create(new(epic, kind, blocked_by, parent)).await.unwrap().id)
}

async fn build(store: &Store, epic: &str, blocked_by: &[&str]) -> String {
    make(store, epic, Kind::Build, blocked_by, None).await
}

const EVERY: [Kind; 5] = [Kind::Scope, Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike];

fn to(store: &Store, id: &str) -> WavesTo {
    store.waves_to(SYS, id, &EVERY).unwrap()
}

/// The answer as `(id, placement)` in yield order.
fn shape(w: &WavesTo) -> Vec<(String, Placement)> {
    w.waves.placed.iter().map(|p| (p.summary.id.to_string(), p.placement)).collect()
}

fn at(id: &str, p: Placement) -> (String, Placement) {
    (id.to_string(), p)
}

fn wave(n: u32) -> Placement {
    Placement::Wave { n }
}

/// A chain A ← B ← C: waves to C are A, B, C in waves 1, 2, 3 — nothing
/// else of the shortcode; waves to B stop at B; and finishing A leaves it
/// in the answer, done, the rest one left.
#[tokio::test]
async fn a_chain_is_its_waves_and_nothing_else() {
    let store = Store::new();
    make(&store, "CH", Kind::Scope, &[], None).await;
    let a = build(&store, "CH", &[]).await;
    let b = build(&store, "CH", &[&a]).await;
    let c = build(&store, "CH", &[&b]).await;
    let _other = build(&store, "CH", &[&a]).await;
    let w = to(&store, &c);
    assert_eq!(shape(&w), [at(&a, wave(1)), at(&b, wave(2)), at(&c, wave(3))]);
    assert_eq!((w.waves.waves.clone(), w.waves.missing, w.waves.done), (vec![1, 1, 1], 0, 0));
    assert!(w.cycles.is_empty());
    assert_eq!(shape(&to(&store, &b)), [at(&a, wave(1)), at(&b, wave(2))]);
    assert_eq!(shape(&to(&store, &a)), [at(&a, wave(1))], "nothing before it: alone in wave 1");

    store.set_status(&a, Status::Complete).await.unwrap();
    assert_eq!(shape(&to(&store, &c)), [at(&b, wave(1)), at(&c, wave(2)), at(&a, Placement::Done)]);
}

/// A diamond — D waits on B and C, both on A — is A, then B and C side by
/// side (the store's order), then D alone; a longer side sets D's wave.
#[tokio::test]
async fn a_diamond_meets_in_one_last_wave() {
    let store = Store::new();
    make(&store, "DI", Kind::Scope, &[], None).await;
    let a = build(&store, "DI", &[]).await;
    let b = build(&store, "DI", &[&a]).await;
    let c = build(&store, "DI", &[&a]).await;
    let d = build(&store, "DI", &[&b, &c]).await;
    assert_eq!(shape(&to(&store, &d)), [at(&a, wave(1)), at(&b, wave(2)), at(&c, wave(2)), at(&d, wave(3))]);

    let x = build(&store, "DI", &[&c]).await;
    let e = build(&store, "DI", &[&b, &x]).await;
    let w = to(&store, &e);
    assert_eq!(shape(&w), [at(&a, wave(1)), at(&b, wave(2)), at(&c, wave(2)), at(&x, wave(3)), at(&e, wave(4))]);
    assert_eq!(w.waves.waves, vec![1, 2, 1, 1]);
}

/// A done blocker is in the answer, done, and not followed past: what it
/// waited on (open or not) holds nothing back. A blocker in another
/// shortcode is followed.
#[tokio::test]
async fn done_blockers_are_done_and_not_followed() {
    let store = Store::new();
    make(&store, "DN", Kind::Scope, &[], None).await;
    make(&store, "XS", Kind::Scope, &[], None).await;
    let far = build(&store, "DN", &[]).await;
    let a = build(&store, "DN", &[&far]).await;
    let ext = build(&store, "XS", &[]).await;
    let b = build(&store, "DN", &[&a, &ext]).await;
    store.set_status(&a, Status::Complete).await.unwrap();
    let w = to(&store, &b);
    assert_eq!(shape(&w), [at(&ext, wave(1)), at(&b, wave(2)), at(&a, Placement::Done)]);
    assert_eq!(w.waves.done, 1);
    assert_eq!(w.waves.placed[1].waiting_on, vec![ext.clone()], "the target waits on its open blocker");

    // A done target is done, alone.
    store.set_status(&b, Status::Superseded).await.unwrap();
    assert_eq!(shape(&to(&store, &b)), [at(&b, Placement::Done)]);
}

/// A scope's open descendants are part of it — at any depth, with what they
/// wait on wherever it is — and it sits alone after all of them; its done
/// and doc descendants are not. A build's children are not gathered.
#[tokio::test]
async fn a_scope_gathers_its_open_descendants() {
    let store = Store::new();
    let root = make(&store, "SC", Kind::Scope, &[], None).await;
    make(&store, "XT", Kind::Scope, &[], None).await;
    let ext = build(&store, "XT", &[]).await;
    // ORG-5: the scope roots a shortcode of its own, hosted under SC-1.
    let s = make(&store, "SS", Kind::Scope, &[], Some(&root)).await;
    let b1 = make(&store, "SS", Kind::Build, &[], Some(&s)).await;
    let e = make(&store, "SS", Kind::Execution, &[], Some(&s)).await;
    let b2 = make(&store, "SS", Kind::Build, &[&ext], Some(&e)).await;
    let b3 = make(&store, "SS", Kind::Build, &[&b2], Some(&e)).await;
    let done = make(&store, "SS", Kind::Build, &[], Some(&s)).await;
    store.set_status(&done, Status::Complete).await.unwrap();
    let doc = make(&store, "SS", Kind::Doc, &[], Some(&s)).await;
    let _beside = make(&store, "SC", Kind::Build, &[], Some(&root)).await;

    let w = to(&store, &s);
    let ids: Vec<&str> = w.waves.placed.iter().map(|p| p.summary.id.as_str()).collect();
    assert!(!ids.contains(&done.as_str()) && !ids.contains(&doc.as_str()), "{ids:?}");
    assert_eq!(
        shape(&w),
        [at(&b1, wave(1)), at(&e, wave(1)), at(&ext, wave(1)), at(&b2, wave(2)), at(&b3, wave(3)), at(&s, wave(4))]
    );
    assert_eq!(w.waves.waves, vec![3, 1, 1, 1], "the scope alone in the last wave");

    // `kinds` filters what is yielded, not what is followed — the target
    // always shows.
    let builds = store.waves_to(SYS, &s, &[Kind::Build]).unwrap();
    assert_eq!(shape(&builds), [at(&b1, wave(1)), at(&ext, wave(1)), at(&b2, wave(2)), at(&b3, wave(3)), at(&s, wave(4))]);

    // A build's children are its own business.
    let kid = make(&store, "SS", Kind::Build, &[], Some(&b1)).await;
    let w = to(&store, &b1);
    assert!(!w.waves.placed.iter().any(|p| p.summary.id == kid));

    // Only what the caller sees is yielded; the rest still counts.
    let viewer = super::tests_reach::viewer_of(&["SC", "SS"]);
    let seen = store.waves_to(&viewer, &s, &EVERY).unwrap();
    assert!(!seen.waves.placed.iter().any(|p| p.summary.id == ext));
    assert_eq!(seen.waves.placed.last().unwrap().placement, wave(4));
}

/// A `blocked_by` cycle (only a hand edit makes one) terminates: it is named
/// — each cycle once, its ids in the store's order — and everything on and
/// behind it, the target included, is missing.
#[tokio::test]
async fn a_cycle_is_named_and_does_not_hang() {
    let store = Store::new();
    make(&store, "CY", Kind::Scope, &[], None).await;
    let a = build(&store, "CY", &[]).await; // CY-2
    let b = build(&store, "CY", &[&a]).await; // CY-3
    let selfish = build(&store, "CY", &[]).await; // CY-4
    let c = build(&store, "CY", &[&b, &selfish]).await; // CY-5
    let free = build(&store, "CY", &[]).await; // CY-6
    let d = build(&store, "CY", &[&c, &free]).await; // CY-7
    {
        let mut cache = store.tickets.write().unwrap();
        for (id, blocked_by) in [(&a, vec![b.clone()]), (&selfish, vec![selfish.clone()])] {
            let key = cache.at[id.as_str()].clone();
            cache.order.get_mut(&key).unwrap().blocked_by = blocked_by;
        }
        cache.place_all();
    }
    let w = to(&store, &d);
    assert_eq!(w.cycles, vec![vec![a.clone(), b.clone()], vec![selfish.clone()]]);
    assert_eq!(
        shape(&w),
        [
            at(&free, wave(1)),
            at(&a, Placement::MissingBlocker),
            at(&b, Placement::MissingBlocker),
            at(&selfish, Placement::MissingBlocker),
            at(&c, Placement::MissingBlocker),
            at(&d, Placement::MissingBlocker),
        ],
        "the target last in the missing lane"
    );
    assert_eq!((w.waves.waves.clone(), w.waves.missing), (vec![1], 5));
}

/// An id that names no ticket, and a doc target.
#[tokio::test]
async fn unknown_ids_and_docs_are_refused() {
    let store = Store::new();
    make(&store, "NO", Kind::Scope, &[], None).await;
    let doc = make(&store, "NO", Kind::Doc, &[], None).await;
    assert!(matches!(store.waves_to(SYS, "NO-99", &EVERY), Err(StoreError::NotFound(_))));
    assert!(matches!(store.waves_to(SYS, &doc, &EVERY), Err(StoreError::DocHasNoLifecycle { .. })));
    assert!(matches!(store.waves_under(SYS, "NO-99", &EVERY), Err(StoreError::NotFound(_))));
}

/// A ticket's subtree's board: it and everything under it, at any depth and
/// across a shortcode hosted there, placed over the whole graph; nothing
/// beside it.
#[tokio::test]
async fn waves_under_is_the_subtree_board() {
    let store = Store::new();
    let root = make(&store, "UN", Kind::Scope, &[], None).await;
    let beside = make(&store, "UN", Kind::Build, &[], Some(&root)).await;
    // ORG-5: the scope roots a shortcode of its own, hosted under UN-1.
    let s = make(&store, "US", Kind::Scope, &[], Some(&root)).await;
    let e = make(&store, "US", Kind::Execution, &[], Some(&s)).await;
    let b1 = make(&store, "US", Kind::Build, &[&beside], Some(&e)).await;
    let b2 = make(&store, "US", Kind::Gap, &[], Some(&s)).await;
    let hosted = make(&store, "HO", Kind::Scope, &[], Some(&s)).await;
    let h = make(&store, "HO", Kind::Spike, &[&b1], Some(&hosted)).await;

    let w = store.waves_under(SYS, &s, &super::waves::DEFAULT_KINDS).unwrap();
    let got: Vec<(String, Placement)> = w.placed.iter().map(|p| (p.summary.id.to_string(), p.placement)).collect();
    assert_eq!(got, [at(&b2, wave(1)), at(&b1, wave(2)), at(&h, wave(3))]);
    assert_eq!(w.placed[1].waiting_on, vec![beside.clone()], "a blocker off the subtree still places it");

    let all = store.waves_under(SYS, &s, &EVERY).unwrap();
    let ids: Vec<&str> = all.placed.iter().map(|p| p.summary.id.as_str()).collect();
    for id in [&s, &e, &hosted, &b1, &b2, &h] {
        assert!(ids.contains(&id.as_str()), "{id} in {ids:?}");
    }
    assert!(!ids.contains(&beside.as_str()) && !ids.contains(&root.as_str()));
}
