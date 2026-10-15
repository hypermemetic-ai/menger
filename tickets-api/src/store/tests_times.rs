//! FILTER-27: the times — each ticket's birth and status-since — and what
//! `created <window>` and `[status S] for over|under D` answer: held to the
//! rule ticket by ticket (windows in a zone, the pre-log fallback), kept by
//! every write as a fresh reading has them, checked by `--verify-derived`,
//! and moving a watch as time passes with no write.

use super::history::{Clock, Log};
use crate::address::ShortcodeId;
use super::tests_filtered::holds_of_row;
use super::tests_ready::Rng;
use super::*;
use crate::filter::{Field, Filter, History};
use crate::types::{Transition, TransitionField, TransitionSource};
use roaring::RoaringBitmap;

const DAY: i64 = 86_400;

fn pred(text: &str) -> History {
    match Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).groups[0].preds[0].field.clone() {
        Field::History(h) => h,
        other => panic!("{text} is no history predicate: {other:?}"),
    }
}

fn at(now: i64) -> Clock {
    Clock { fixed: Some(now), ..Clock::default() }
}

fn row(epic: &str, seq: u32, status: Status, created: i64, updated: i64) -> Row {
    Row {
        id: format!("{epic}-{seq}").into(),
        epic: epic.into(),
        shortcode: epic.into(),
        address: format!("hypermemetic:{epic}-{seq}"),
        seq,
        title: String::new(),
        status,
        kind: Kind::Build,
        severity: None,
        parent: None,
        child_count: 0,
        blocked_by: Vec::new(),
        unlocks: Vec::new(),
        commits: Vec::new(),
        path: None,
        placement: None,
        labels: Vec::new(),
        created_unix: created,
        updated_unix: updated,
    }
}

fn transition(id: u64, ticket: &str, from: Option<Status>, to: Status, at_unix: i64) -> Transition {
    Transition {
        id,
        ticket: ticket.into(),
        field: TransitionField::Status,
        from: from.map(|s| s.as_str().into()),
        to: to.as_str().into(),
        at_unix,
        actor: "anonymous".into(),
        name: None,
        source: TransitionSource::Api,
        message: None,
        cause: None,
        cause_ticket: None,
    }
}

/// A shortcode's root — its first ticket is a scope.
fn scope(epic: &str) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("{epic} root"),
        kind: Kind::Scope,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: String::new(),
    }
}

fn ids(store: &Store, text: &str, clock: Clock) -> Vec<String> {
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(clock);
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(&q, None, None, None).unwrap().tickets.into_iter().map(|t| t.id.into_string()).collect()
}

/// Random tickets — born over two months, some from before the log (no
/// transition, updated before or after it began), the rest with status
/// transitions since — and every `created` window and every `for`, read at
/// random moments in UTC and in Berlin, is exactly the tickets the rule
/// passes, ticket by ticket.
#[test]
fn created_and_for_answer_as_the_rule() {
    let mut rng = Rng(0xF1_17E2_7);
    let base = 1_791_799_200; // 2026-10-12T06:00Z, a Monday
    let began = base - 3 * DAY;
    let statuses = [Status::Pending, Status::Ready, Status::Coding, Status::InReview, Status::Complete, Status::Canceled];
    for round in 0..6 {
        let mut log = Log::began_at(Some(began));
        let mut rows: Vec<Row> = Vec::new();
        let mut own: BTreeMap<String, Vec<Transition>> = BTreeMap::new();
        let mut next_id = 1u64;
        for seq in 1..=120u32 {
            let epic = ["A", "B", "C"][rng.below(3)];
            let created = base - 60 * DAY + rng.below(60 * DAY as usize) as i64;
            let updated = created + rng.below(20 * DAY as usize) as i64;
            let mut status = statuses[rng.below(statuses.len())];
            let mut r = row(epic, seq, status, created, updated);
            log.retime(&r);
            // Born after the log began: a birth transition, then moves.
            let mut ts = Vec::new();
            if created >= began || rng.below(3) == 0 {
                let mut when = created.max(began);
                ts.push(transition(next_id, &r.id, None, status, when));
                next_id += 1;
                for _ in 0..rng.below(3) {
                    let to = statuses[rng.below(statuses.len())];
                    when += rng.below(2 * DAY as usize) as i64;
                    ts.push(transition(next_id, &r.id, Some(status), to, when));
                    next_id += 1;
                    status = to;
                }
                r.status = status;
                r.updated_unix = r.updated_unix.max(when);
            }
            // The row's write and the log's appends, in either order.
            if rng.below(2) == 0 {
                log.retime(&r);
            }
            for t in &ts {
                log.append(t, &OrderKey { shortcode: ShortcodeId::from(r.epic.clone()), seq });
            }
            log.retime(&r);
            own.insert(r.id.to_string(), ts);
            rows.push(r);
        }
        assert!(log.check_times(rows.iter()).is_empty(), "round {round}: {:?}", log.check_times(rows.iter()));
        let texts = [
            "created today",
            "created yesterday",
            "created this week",
            "created within 3d",
            "created within 1mo",
            "created since 2026-10-01",
            "created before 2026-09-20T12:00",
            "created between 2026-09-25 and 2026-10-03",
            "created before 2w ago",
            "created between 3w ago and 1w ago",
            "created since 36h ago",
            "for over 2w",
            "for under 3d",
            "for over 1mo",
            "status Pending for over 1w",
            "status \"In Review\" for under 2d",
            "category in (ready, active, review) for over 1w",
            "category done for under 1w",
        ];
        for text in texts {
            let p = pred(text);
            for zone in ["UTC", "Europe/Berlin", "America/Los_Angeles"] {
                let clock = at(base + rng.below(3 * DAY as usize) as i64 - DAY).zoned(Some(zone)).unwrap();
                let got = log.holds(&p, &clock);
                let mut want: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
                for r in &rows {
                    if holds_of_row(&p, r, &own[r.id.as_str()], Some(began), &clock) {
                        want.entry(r.shortcode.clone()).or_default().insert(r.seq);
                    }
                }
                assert_eq!(got, want, "round {round}: `{text}` at {:?} in {zone}", clock.fixed);
            }
        }
    }
}

/// A calendar window is the zone's days: born at 23:30 UTC is today in
/// Berlin but yesterday in UTC, read at 06:00 UTC the next morning.
#[test]
fn created_windows_are_read_in_the_zone() {
    let mut log = Log::began_at(None);
    let born = 1_791_847_800; // 2026-10-12T23:30Z = 2026-10-13T01:30 Berlin
    log.retime(&row("A", 1, Status::Pending, born, born));
    let morning = born + 6 * 3_600 + 30 * 60; // 2026-10-13T06:00Z
    let held = |text: &str, zone: &str| !log.holds(&pred(text), &at(morning).zoned(Some(zone)).unwrap()).is_empty();
    assert!(held("created today", "Europe/Berlin"));
    assert!(!held("created today", "UTC"));
    assert!(held("created yesterday", "UTC"));
    assert!(held("created between 2026-10-13 and 2026-10-13", "Europe/Berlin"));
    assert!(!held("created between 2026-10-13 and 2026-10-13", "UTC"));
    assert!(held("created since 2026-10-13T01:00", "Europe/Berlin"));
    assert!(held("created within 7h", "UTC") && !held("created within 6h", "UTC"), "`within` is the same everywhere");
}

/// A ticket from before the log: its status taken as held since its last
/// update — or, updated since the log began, since the log began — and
/// never before its birth; `history` says so ("status since at least …":
/// not exact). Its first transition makes it exact.
#[test]
fn a_ticket_from_before_the_log_holds_its_status_since_at_least() {
    let began = 1_791_000_000;
    let mut log = Log::began_at(Some(began));
    log.retime(&row("A", 1, Status::Pending, began - 40 * DAY, began - 20 * DAY));
    log.retime(&row("A", 2, Status::Pending, began - 40 * DAY, began + 2 * DAY));
    log.retime(&row("A", 3, Status::Pending, began - 40 * DAY, 0));
    let t = |log: &Log, seq| log.timed(&ShortcodeId::from("A"), seq).unwrap();
    assert_eq!((t(&log, 1).since, t(&log, 1).exact), (began - 20 * DAY, false), "its last update");
    assert_eq!((t(&log, 2).since, t(&log, 2).exact), (began, false), "updated since the log began: since the log began, at least");
    assert_eq!((t(&log, 3).since, t(&log, 3).exact), (began - 40 * DAY, false), "no update on record: its birth");
    let now = at(began + 3 * DAY);
    let over = |log: &Log, text: &str| log.holds(&pred(text), &now).get(&ShortcodeId::from("A")).map(|b| b.iter().collect::<Vec<_>>()).unwrap_or_default();
    assert_eq!(over(&log, "status Pending for over 2w"), [1, 3]);
    assert_eq!(over(&log, "for under 2w"), [2]);
    log.append(&transition(1, "A-1", Some(Status::Pending), Status::Ready, began + DAY), &OrderKey { shortcode: ShortcodeId::from("A"), seq: 1 });
    assert_eq!((t(&log, 1).since, t(&log, 1).exact, t(&log, 1).status), (began + DAY, true, Status::ALL.iter().position(|x| *x == Status::Ready).unwrap() as u8), "moved: exact, in Ready");
    assert_eq!(over(&log, "status Pending for over 2w"), [3]);
}

/// The store's writes keep the times as a fresh reading has them — a birth,
/// a status change, a cascade, a kind change into a doc, an amend — and
/// `history` reports each ticket's status-since; `--verify-derived` holds
/// them on a reopened file.
#[tokio::test]
async fn every_write_keeps_the_times_and_verify_checks_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("times.db");
    let store = Store::open(&path).unwrap();
    let t = |title: &str, parent: Option<&str>| NewTicket {
        epic: "T".into(),
        title: title.into(),
        kind: Kind::Build,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(String::from),
        path: None,
        body: String::new(),
    };
    store.create(scope("T")).await.unwrap();
    store.create(t("one", None)).await.unwrap();
    store.create(t("two", Some("T-2"))).await.unwrap();
    store.create(t("three", Some("T-2"))).await.unwrap();
    store.set_status("T-3", Status::Ready).await.unwrap();
    store.set_status("T-2", Status::Canceled).await.unwrap();
    store.amend("T-4", None, Some("a body".into())).await.unwrap();
    let fresh = |store: &Store| {
        let cache = store.tickets.read().unwrap();
        cache.log.check_times(cache.order.values())
    };
    assert!(fresh(&store).is_empty(), "{:?}", fresh(&store));
    for id in ["T-1", "T-2", "T-3", "T-4"] {
        let h = store.history(id).unwrap();
        let last = h.own.iter().rev().find(|t| t.field == TransitionField::Status).unwrap();
        assert_eq!(h.since, Some((last.at_unix, true)), "{id}");
    }
    drop(store);
    let store = Store::open(&path).unwrap();
    assert!(fresh(&store).is_empty());
    drop(store);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { .. } => {}
        other => panic!("{other:?}"),
    }
}

/// On a file whose tickets predate the log (their transitions gone, their
/// times set back by hand), the acceptance's three: `created today`, `left
/// Pending this week`, `status Pending for over 2w` — each ticket by its
/// times and its own history.
#[tokio::test]
async fn the_acceptance_filters_on_a_file_from_before_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let store = Store::open(&path).unwrap();
    let n = |title: &str| NewTicket {
        epic: "O".into(),
        title: title.into(),
        kind: Kind::Build,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: String::new(),
    };
    store.create(scope("O")).await.unwrap();
    for title in ["old pending", "old pending edited", "old ready", "new"] {
        store.create(n(title)).await.unwrap();
    }
    store.set_status("O-4", Status::Ready).await.unwrap();
    drop(store);
    let now = chrono::Utc::now().timestamp();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("DELETE FROM transitions", []).unwrap();
        conn.execute("UPDATE transitions_began SET at_unix = ?1", [now - 2 * DAY]).unwrap();
        for (id, created, updated) in [("O-1", now - 40 * DAY, now - 40 * DAY), ("O-2", now - 40 * DAY, now - 30 * DAY), ("O-3", now - 40 * DAY, now - DAY), ("O-4", now - 40 * DAY, now - 30 * DAY)] {
            conn.execute("UPDATE tickets SET created_unix = ?2, updated_unix = ?3 WHERE id = ?1", rusqlite::params![id, created, updated]).unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    // O-4 had been Ready since before the log; it moves now.
    store.set_status("O-4", Status::Pending).await.unwrap();
    store.set_status("O-5", Status::Coding).await.unwrap();
    let clock = at(now + 60);
    assert_eq!(ids(&store, "created today", clock.clone()), ["O-5"]);
    assert_eq!(ids(&store, "left Pending this week", clock.clone()), ["O-5"]);
    assert_eq!(ids(&store, "kind build and status Pending for over 2w", clock.clone()), ["O-2"], "O-3's status since at least the log's start");
    assert_eq!(ids(&store, "status Pending for under 2w", clock.clone()), ["O-3", "O-4"]);
    let h = store.history("O-2").unwrap();
    assert_eq!(h.since, Some((now - 30 * DAY, false)), "status since at least its last update");
    assert_eq!(store.history("O-3").unwrap().since, Some((now - 2 * DAY, false)));
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}

/// Time passes with no write: a ticket crossing the length enters `for over
/// 1h`, and leaves `created within 1h` — `aged` says so, the boundary is
/// where the first crosses, and a watch sleeps until it.
#[tokio::test]
async fn crossing_a_length_moves_a_watch_with_no_write() {
    let store = Store::new();
    let n = NewTicket {
        epic: "W".into(),
        title: "waits".into(),
        kind: Kind::Build,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: String::new(),
    };
    store.create(scope("W")).await.unwrap();
    store.create(n).await.unwrap();
    let born = store.row("W-2").unwrap().0.created_unix;
    let since = store.history("W-2").unwrap().since.unwrap().0;
    let over = Lens::parse("kind build and status Pending for over 1h").unwrap().with_clock(at(born));
    assert_eq!(store.next_boundary(&over), Some(since + 3_600 + 1));
    assert!(store.aged(&over, born, since + 3_600, None).is_empty(), "not yet over the hour");
    assert_eq!(store.aged(&over, born, since + 3_601, None), [Membership::Entered { id: "W-2".into(), shortcode: ShortcodeId::from("W"), seq: 2 }]);
    let fresh = Lens::parse("kind build and status Pending and created within 1h").unwrap().with_clock(at(born));
    assert_eq!(store.next_boundary(&fresh), Some(born + 3_600 + 1));
    assert_eq!(store.aged(&fresh, born, born + 2 * 3_600, None), [Membership::Left { id: "W-2".into(), shortcode: ShortcodeId::from("W"), seq: 2 }]);
    // A watch sleeps until the boundary (at most its cap).
    let watcher = store.watcher(over.clone());
    let wait = watcher.wait(&store, std::time::Duration::from_secs(7_200));
    assert!(wait.as_secs() > 3_500 && wait.as_secs() <= 3_601, "{wait:?}");
    assert_eq!(store.next_boundary(&Lens::parse("created since 2026-01-01").unwrap()), None, "a fixed window never moves");
    let day = Lens::parse("created today").unwrap().with_clock(at(born));
    assert!(store.next_boundary(&day).is_some_and(|t| t > born && t <= born + DAY), "tomorrow's start");
    // A status write moves `since`: the boundary follows it.
    store.set_status("W-2", Status::Ready).await.unwrap();
    let moved = store.history("W-2").unwrap().since.unwrap().0;
    assert_eq!(store.next_boundary(&over), Some(moved + 3_600 + 1));
}
