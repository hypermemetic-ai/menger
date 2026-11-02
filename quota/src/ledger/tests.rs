use super::*;
use std::sync::Arc;

fn open() -> (tempfile::TempDir, Ledger) {
    let dir = tempfile::tempdir().unwrap();
    let l = Ledger::open(&dir.path().join("quota.db"), &Plan::built_in()).unwrap();
    (dir, l)
}

const T0: i64 = 1_793_609_096;

#[test]
fn a_fresh_file_takes_the_plan_and_keeps_its_own_after() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.db");
    let l = Ledger::open(&path, &Plan::built_in()).unwrap();
    assert_eq!(l.settings().plan, "free-1k");
    assert_eq!(l.settings().concurrent.get("media.bytes"), Some(&2));
    l.set(&Holder::Default, "media.bytes", Some(Cap::At(7)), "idp:admin").unwrap();
    drop(l);
    let other = Plan::parse(r#"{"name": "tight", "default": {"media.bytes": "5 GB"}}"#).unwrap();
    let l = Ledger::open(&path, &other).unwrap();
    assert_eq!(l.settings().plan, "free-1k", "a file that has a plan keeps it");
    let lines = l.usage(&Who::plain("key:a"), T0).unwrap();
    let media = lines.iter().find(|x| x.source.name == "media.bytes").unwrap();
    assert_eq!((media.cap, &media.from), (Cap::At(7), &CapFrom::Default), "the admin's edit stands");
}

#[test]
fn reserve_commit_release_count_once() {
    let (_d, l) = open();
    let who = Who::plain("key:a");
    let r = l.reserve(&who, "tickets.bytes", 1_000, T0, None).unwrap();
    assert_eq!(r.remaining, Some(50_000_000 - 1_000));
    let line = |l: &Ledger| l.usage(&who, T0).unwrap().into_iter().find(|x| x.source.name == "tickets.bytes").unwrap();
    assert_eq!((line(&l).used, line(&l).held), (0, 1_000), "held, not used");
    let c = l.commit(&r.ticket, Some(600)).unwrap();
    assert_eq!((c.amount, c.used), (600, 600), "the actual amount, less than reserved");
    assert_eq!((line(&l).used, line(&l).held), (600, 0));
    assert!(matches!(l.commit(&r.ticket, None), Err(LedgerError::NoHold(_))), "a hold commits once");
    let r2 = l.reserve(&who, "tickets.bytes", 50, T0, None).unwrap();
    assert!(matches!(l.commit(&r2.ticket, Some(51)), Err(LedgerError::PastHold { reserved: 50, amount: 51, .. })));
    l.release(&r2.ticket).unwrap();
    assert_eq!((line(&l).used, line(&l).held), (600, 0), "a release counts nothing");
    assert_eq!(l.credit("key:a", "tickets.bytes", 1_000, T0).unwrap(), 0, "a credit never goes below nothing");
}

#[test]
fn over_the_cap_is_refused_typed_with_the_numbers() {
    let (_d, l) = open();
    l.set(&Holder::Principal("key:a".into()), "media.bytes", Some(Cap::At(100)), "admin").unwrap();
    let who = Who::plain("key:a");
    let r = l.reserve(&who, "media.bytes", 60, T0, None).unwrap();
    l.commit(&r.ticket, None).unwrap();
    let _held = l.reserve(&who, "media.bytes", 30, T0, None).unwrap();
    let e = l.reserve(&who, "media.bytes", 11, T0, None).unwrap_err();
    assert_eq!(e, Refusal::OverAllowance { source: "media.bytes".into(), used: 90, cap: 100, asked: 11 });
    assert!(l.reserve(&who, "media.bytes", 10, T0, None).is_ok(), "exactly to the cap fits");
    l.set(&Holder::Principal("key:a".into()), "media.bytes", Some(Cap::Unlimited), "admin").unwrap();
    assert_eq!(l.reserve(&who, "media.bytes", 1, T0, None).unwrap_err(), Refusal::TooManyAtOnce { source: "media.bytes".into(), open: 2, max: 2 }, "two uploads in flight: the third waits");
}

#[test]
fn the_effective_cap_is_own_then_teams_largest_then_default() {
    let (_d, l) = open();
    let src = "docs.bytes";
    let cap = |who: &Who| l.usage(who, T0).unwrap().into_iter().find(|x| x.source.name == src).map(|x| (x.cap, x.from)).unwrap();
    let mut who = Who { principal: "idp:p".into(), teams: vec!["a".into(), "b".into()], exempt: false };
    assert_eq!(cap(&who), (Cap::At(200_000_000), CapFrom::Default));
    l.set(&Holder::Team("a".into()), src, Some(Cap::At(5)), "admin").unwrap();
    l.set(&Holder::Team("b".into()), src, Some(Cap::At(9)), "admin").unwrap();
    assert_eq!(cap(&who), (Cap::At(9), CapFrom::Team("b".into())), "the largest of their teams'");
    l.set(&Holder::Team("a".into()), src, Some(Cap::Unlimited), "admin").unwrap();
    assert_eq!(cap(&who), (Cap::Unlimited, CapFrom::Team("a".into())), "unlimited is the largest");
    l.set(&Holder::Principal("idp:p".into()), src, Some(Cap::At(3)), "admin").unwrap();
    assert_eq!(cap(&who), (Cap::At(3), CapFrom::Own), "their own wins, even below a team's");
    l.set(&Holder::Principal("idp:p".into()), src, None, "admin").unwrap();
    who.teams.clear();
    assert_eq!(cap(&who), (Cap::At(200_000_000), CapFrom::Default), "cleared: it falls back");
    l.set(&Holder::Default, src, None, "admin").unwrap();
    assert_eq!(cap(&who), (Cap::Unlimited, CapFrom::Default), "no default: unlimited");
    who.exempt = true;
    assert_eq!(cap(&who), (Cap::Unlimited, CapFrom::Exempt));
}

#[test]
fn a_cap_below_use_blocks_new_writes_and_deletes_nothing() {
    let (_d, l) = open();
    let who = Who::plain("key:a");
    let r = l.reserve(&who, "tickets.bytes", 500, T0, None).unwrap();
    l.commit(&r.ticket, None).unwrap();
    l.set(&Holder::Principal("key:a".into()), "tickets.bytes", Some(Cap::At(100)), "admin").unwrap();
    assert!(matches!(l.reserve(&who, "tickets.bytes", 1, T0, None), Err(Refusal::OverAllowance { used: 500, cap: 100, .. })));
    assert_eq!(l.totals("tickets.bytes").unwrap(), vec![("key:a".into(), 500)]);
}

#[test]
fn a_day_counter_starts_again_each_day() {
    let (_d, l) = open();
    let who = Who::plain("key:a");
    for _ in 0..300 {
        let r = l.reserve(&who, "media.fetch.calls", 1, T0, None).unwrap();
        l.commit(&r.ticket, None).unwrap();
    }
    assert!(matches!(l.reserve(&who, "media.fetch.calls", 1, T0, None), Err(Refusal::OverAllowance { used: 300, cap: 300, asked: 1, .. })));
    let tomorrow = T0 + 86_400;
    assert!(l.reserve(&who, "media.fetch.calls", 1, tomorrow, None).is_ok(), "a new day, a new count");
}

#[test]
fn an_abandoned_reservation_expires_and_is_swept() {
    let (_d, l) = open();
    let who = Who::plain("key:a");
    l.set(&Holder::Principal("key:a".into()), "media.bytes", Some(Cap::At(100)), "admin").unwrap();
    let r = l.reserve(&who, "media.bytes", 100, T0, None).unwrap();
    assert!(l.reserve(&who, "media.bytes", 1, T0, None).is_err(), "held");
    let later = r.expires;
    assert!(l.reserve(&who, "media.bytes", 100, later, None).is_ok(), "expired: swept by the next reserve");
    assert!(matches!(l.commit(&r.ticket, None), Err(LedgerError::NoHold(_))), "an expired hold commits nothing");
    assert_eq!(l.sweep(later + 10_000).unwrap(), 1);
    assert_eq!(l.open_holds(), 0);
}

#[test]
fn the_box_guard_refuses_the_capped_never_the_exempt() {
    let (_d, l) = open();
    let who = Who::plain("key:a");
    assert!(l.reserve(&who, "tickets.bytes", 1, T0, Some(84.9)).is_ok(), "below the line");
    assert_eq!(l.reserve(&who, "tickets.bytes", 1, T0, Some(85.0)).unwrap_err(), Refusal::BoxFull { used_percent: 85.0, full_percent: 85.0 });
    let admin = Who { exempt: true, ..Who::plain("idp:admin") };
    assert!(l.reserve(&admin, "tickets.bytes", 1, T0, Some(99.0)).is_ok(), "an admin of * still writes");
    assert!(l.reserve(&who, "tickets.bytes", 1, T0, None).is_ok(), "a disk that cannot be read refuses nothing");
    assert!(l.reserve(&who, "media.egress.bytes", 1, T0, Some(99.0)).is_ok(), "what is served stores nothing: never box_full");
}

#[test]
fn unknown_sources_are_refused() {
    let (_d, l) = open();
    assert_eq!(l.reserve(&Who::plain("key:a"), "media.nothing", 1, T0, None).unwrap_err(), Refusal::UnknownSource { source: "media.nothing".into() });
    assert!(matches!(l.set(&Holder::Default, "x", None, "a"), Err(LedgerError::UnknownSource(_))));
}

/// The concurrency test: 64 threads reserve near the cap at once; exactly as
/// many pass as fit, and the held total never passes the cap.
#[test]
fn parallel_reserves_near_the_cap_never_double_count() {
    let (_d, l) = open();
    let l = Arc::new(l);
    l.set(&Holder::Principal("key:a".into()), "tickets.bytes", Some(Cap::At(1_000)), "admin").unwrap();
    let r = l.reserve(&Who::plain("key:a"), "tickets.bytes", 300, T0, None).unwrap();
    l.commit(&r.ticket, None).unwrap();
    // 700 left: 64 asks of 25 — 28 fit.
    let threads: Vec<_> = (0..64)
        .map(|_| {
            let l = Arc::clone(&l);
            std::thread::spawn(move || l.reserve(&Who::plain("key:a"), "tickets.bytes", 25, T0, None))
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let passed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(passed.len(), 28, "exactly what fits");
    assert!(results.iter().filter_map(|r| r.as_ref().err()).all(|e| matches!(e, Refusal::OverAllowance { cap: 1_000, asked: 25, .. })));
    // Commit them all in parallel too: used is the sum, once.
    let threads: Vec<_> = passed
        .iter()
        .map(|r| {
            let (l, t) = (Arc::clone(&l), r.ticket.clone());
            std::thread::spawn(move || l.commit(&t, None).unwrap())
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(l.totals("tickets.bytes").unwrap(), vec![("key:a".into(), 1_000)]);
    assert_eq!(l.open_holds(), 0);
}

/// Two ledgers on one file (two processes, in effect) keep it too: the
/// IMMEDIATE transaction serialises them.
#[test]
fn two_connections_on_one_file_never_double_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.db");
    let a = Arc::new(Ledger::open(&path, &Plan::built_in()).unwrap());
    let b = Arc::new(Ledger::open(&path, &Plan::built_in()).unwrap());
    a.set(&Holder::Principal("key:a".into()), "tickets.count", Some(Cap::At(10)), "admin").unwrap();
    let threads: Vec<_> = (0..40)
        .map(|i| {
            let l = if i % 2 == 0 { Arc::clone(&a) } else { Arc::clone(&b) };
            std::thread::spawn(move || l.reserve(&Who::plain("key:a"), "tickets.count", 1, T0, None).is_ok())
        })
        .collect();
    let passed = threads.into_iter().filter(|_| true).map(|t| t.join().unwrap()).filter(|ok| *ok).count();
    assert_eq!(passed, 10);
}
