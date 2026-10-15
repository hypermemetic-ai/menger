//! ACCESS-12: what an open watch hears ([`Store::heard`]) — a change about
//! a hidden ticket never; a visible row redacted as a read's is; a hidden
//! name beside it dropped or blanked; a component of a hidden owner said as
//! gone — and what a moved reach did ([`Store::moved_between`]).

use super::tests_reach::{new, tracker, viewer_of};
use super::*;

/// Every change on `rx` now, as `sight` hears it.
fn heard(s: &Store, sight: &WatchSight, rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    let mut out = Vec::new();
    while let Ok(c) = rx.try_recv() {
        if let Some(c) = s.heard(sight, c) {
            out.push(c);
        }
    }
    out
}

/// What a change names, as JSON text.
fn text(changes: &[Change]) -> String {
    serde_json::to_string(changes).unwrap()
}

#[tokio::test]
async fn a_change_about_a_hidden_ticket_never_reaches_a_narrow_watcher() {
    let s = tracker().await;
    let sight = s.watch_sight(&viewer_of(&["OPEN"])).unwrap();
    let mut rx = s.subscribe();
    s.amend(SYS.permit("SECRET-2"), Some("still secret".into()), Some("classified again".into())).await.unwrap();
    s.create(SYS, new("SECRET", Kind::Build, Some("SECRET-1"), &[], "classified too")).await.unwrap();
    s.set_status(SYS.permit("SECRET-2"), Status::Ready).await.unwrap();
    s.amend(SYS.permit("OPEN-2"), Some("seen".into()), None).await.unwrap();
    let got = heard(&s, &sight, &mut rx);
    let t = text(&got);
    assert!(!t.contains("SECRET-") && !t.contains("classified"), "{t}");
    assert!(got.iter().any(|c| matches!(c, Change::Amended { ticket, .. } if ticket.id.as_str() == "OPEN-2")), "{t}");
    // The system hears all of it.
    let mut rx = s.subscribe();
    s.amend(SYS.permit("SECRET-2"), Some("again".into()), None).await.unwrap();
    let all = s.watch_sight(SYS).unwrap();
    assert!(text(&heard(&s, &all, &mut rx)).contains("SECRET-2"));
}

#[tokio::test]
async fn a_row_event_of_a_visible_ticket_blocked_by_a_hidden_one_is_redacted() {
    let s = tracker().await;
    let sight = s.watch_sight(&viewer_of(&["OPEN"])).unwrap();
    let mut rx = s.subscribe();
    // OPEN-5 waits on OPEN-2 and SECRET-2: born, and each blocker's side.
    s.create(SYS, new("OPEN", Kind::Build, Some("OPEN-1"), &["OPEN-2", "SECRET-2"], "")).await.unwrap();
    // SECRET-3 waits on OPEN-2: OPEN-2's unlocks gained a hidden ticket.
    s.create(SYS, new("SECRET", Kind::Build, Some("SECRET-1"), &["OPEN-2"], "")).await.unwrap();
    let got = heard(&s, &sight, &mut rx);
    let t = text(&got);
    assert!(!t.contains("SECRET-"), "no hidden id, anywhere: {t}");
    let born = got.iter().find_map(|c| match c {
        Change::Created { ticket } => Some(ticket),
        _ => None,
    });
    let born = born.expect("OPEN-5's birth");
    assert_eq!((born.id.as_str(), born.blocked_by.clone(), born.hidden_blocked_by), ("OPEN-5", vec!["OPEN-2".to_string()], 1));
    let unlocks: Vec<(String, String, Vec<String>, u32)> = got
        .iter()
        .filter_map(|c| match c {
            Change::UnlocksChanged { ticket, added } => Some((ticket.id.to_string(), added.clone(), ticket.unlocks.clone(), ticket.hidden_unlocks)),
            _ => None,
        })
        .collect();
    // OPEN-2 gained OPEN-5 (seen) and SECRET-3 (blanked, counted); SECRET-2's is not heard.
    assert_eq!(unlocks.len(), 2, "{unlocks:?}");
    assert_eq!(unlocks[0].1, "OPEN-5");
    assert_eq!((unlocks[1].0.as_str(), unlocks[1].1.as_str(), unlocks[1].3), ("OPEN-2", "", 1), "{unlocks:?}");
    assert!(unlocks[1].2.iter().all(|u| u.starts_with("OPEN-")));
    // A hidden parent is none, said — OPEN-1's (under SECRET-1).
    let mut rx = s.subscribe();
    s.amend(SYS.permit("OPEN-1"), Some("the open root".into()), None).await.unwrap();
    let got = heard(&s, &sight, &mut rx);
    let root = got.iter().find_map(|c| match c {
        Change::Amended { ticket, .. } => Some(ticket),
        _ => None,
    });
    let root = root.expect("OPEN-1 amended");
    assert_eq!((root.parent.clone(), root.parent_hidden), (None, true));
    assert!(!text(&got).contains("SECRET-"), "{}", text(&got));
}

#[tokio::test]
async fn a_component_of_a_hidden_owner_is_said_as_gone_and_references_are_cut() {
    let s = tracker().await;
    let sight = s.watch_sight(&viewer_of(&["OPEN"])).unwrap();
    let mut rx = s.subscribe();
    s.amend(SYS.permit("SECRET-2"), None, Some("```component m-secret\n<template>s2</template>\n```\n".into())).await.unwrap();
    s.amend(SYS.permit("OPEN-2"), None, Some("```component m-open\n<template><m-secret></m-secret></template>\n```\n".into())).await.unwrap();
    let got = heard(&s, &sight, &mut rx);
    let t = text(&got);
    assert!(!t.contains("s2") && !t.contains("SECRET-"), "{t}");
    assert!(got.iter().any(|c| matches!(c, Change::ComponentRemoved { name } if name == "m-secret")), "{t}");
    if let Some(Change::ComponentChanged { component }) = got.iter().find(|c| matches!(c, Change::ComponentChanged { component } if component.name == "m-open")) {
        assert!(!component.references.iter().any(|r| r == "m-secret"), "{component:?}");
    }
}

#[tokio::test]
async fn several_ticket_changes_are_cut_to_the_reach() {
    let s = tracker().await;
    let sight = s.watch_sight(&viewer_of(&["OPEN"])).unwrap();
    let mut rx = s.subscribe();
    let ids = vec!["OPEN-2".to_string(), "SECRET-2".to_string()];
    let selection = Selection::Ids(ids);
    s.update_many(SYS, selection, Update { label: vec!["both".into()], ..Default::default() }).await.unwrap();
    let got = heard(&s, &sight, &mut rx);
    let many = got.iter().find_map(|c| match c {
        Change::UpdatedMany { ids, .. } => Some(ids.clone()),
        _ => None,
    });
    assert_eq!(many, Some(vec!["OPEN-2".to_string()]), "{}", text(&got));
    assert!(!text(&got).contains("SECRET-"), "{}", text(&got));
}

#[tokio::test]
async fn a_moved_reach_says_what_entered_and_left() {
    let s = tracker().await;
    let (open, secret) = (s.watch_sight(&viewer_of(&["OPEN"])).unwrap(), s.watch_sight(&viewer_of(&["SECRET"])).unwrap());
    let ids = |ms: Vec<Membership>| -> Vec<String> {
        ms.into_iter()
            .map(|m| match m {
                Membership::Entered { id, .. } => format!("+{id}"),
                Membership::Left { id, .. } => format!("-{id}"),
                Membership::Recount { shortcode, held } => format!("={shortcode}:{held}"),
            })
            .collect()
    };
    // SECRET holds OPEN (hosted under it): widened, SECRET's two enter.
    assert_eq!(ids(s.moved_between(open.lens(), secret.lens(), None)), ["+SECRET-1", "+SECRET-2"]);
    assert_eq!(ids(s.moved_between(secret.lens(), open.lens(), None)), ["-SECRET-1", "-SECRET-2"]);
    // To everything; within a filter; in one shortcode; unmoved.
    assert_eq!(ids(s.moved_between(open.lens(), None, None)), ["+SECRET-1", "+SECRET-2"]);
    let build = Lens::of(&crate::filter::Filter::parse("kind build").unwrap()).unwrap();
    let (b_open, b_all) = (build.and(open.lens().unwrap()).unwrap(), build.clone());
    assert_eq!(ids(s.moved_between(Some(&b_open), Some(&b_all), None)), ["+SECRET-2"]);
    assert!(s.moved_between(open.lens(), secret.lens(), Some(&ShortcodeId::new("OPEN"))).is_empty());
    assert!(s.moved_between(open.lens(), open.lens(), None).is_empty());
}

#[tokio::test]
async fn a_reach_moving_past_the_threshold_recounts() {
    let s = tracker().await;
    for _ in 0..RECOUNT_OVER {
        s.create(SYS, new("SECRET", Kind::Build, Some("SECRET-1"), &[], "")).await.unwrap();
    }
    let (open, secret) = (s.watch_sight(&viewer_of(&["OPEN"])).unwrap(), s.watch_sight(&viewer_of(&["SECRET"])).unwrap());
    let moved = s.moved_between(open.lens(), secret.lens(), None);
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert!(matches!(&moved[0], Membership::Recount { shortcode, held } if shortcode.as_str() == "SECRET" && *held == RECOUNT_OVER as u64 + 2), "{moved:?}");
}

/// ACCESS-12's measure: many narrow watchers hearing many changes — each
/// change one compile of the reach against the cache, nothing scanned.
#[tokio::test]
async fn many_watchers_hear_many_changes_without_scanning_the_store() {
    let s = tracker().await;
    for i in 0..200 {
        s.create(SYS, new(if i % 2 == 0 { "OPEN" } else { "SECRET" }, Kind::Build, Some(if i % 2 == 0 { "OPEN-1" } else { "SECRET-1" }), &[], "")).await.unwrap();
    }
    let watchers: Vec<WatchSight> = (0..100).map(|_| s.watch_sight(&viewer_of(&["OPEN"])).unwrap()).collect();
    let mut rx = s.subscribe();
    for i in 0..100 {
        s.amend(SYS.permit(&format!("{}-{}", if i % 2 == 0 { "OPEN" } else { "SECRET" }, 5 + i / 2)), Some(format!("t{i}")), None).await.unwrap();
    }
    let mut changes = Vec::new();
    while let Ok(c) = rx.try_recv() {
        changes.push(c);
    }
    let started = std::time::Instant::now();
    let mut relayed = 0usize;
    for w in &watchers {
        for c in &changes {
            relayed += usize::from(matches!(s.heard(w, c.clone()), Some(Change::Amended { .. })));
        }
    }
    let each = started.elapsed() / (watchers.len() * changes.len()) as u32;
    // What the watch asked before (IDENTITY-37's `within`): one `member` a
    // change — the reach parsed and compiled again each time.
    let v = viewer_of(&["OPEN"]);
    let started = std::time::Instant::now();
    for _ in &watchers {
        for c in &changes {
            if let Change::Amended { ticket, .. } = c {
                let _ = s.member(&v, &[ticket.id.to_string()], &Lens::all());
            }
        }
    }
    let before = started.elapsed() / (watchers.len() * changes.len()) as u32;
    eprintln!("ACCESS-12: {} watchers × {} changes: {relayed} relayed, {each:?} a change a watcher (redacted); before, {before:?} (one member, unredacted)", watchers.len(), changes.len());
    assert_eq!(relayed, watchers.len() * 50, "each watcher hears OPEN's half");
    assert!(each < std::time::Duration::from_millis(5), "{each:?} a change: the reach is compiled, not the store walked");
}
