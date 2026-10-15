//! MENGER-13: the transition log, cancellation cascades and their undoing,
//! and the windows a history filter reads — held to their rules, and every
//! structure the store keeps held to a scan after cascades and restores.

use super::history::{Act, Clock, Log};
use crate::address::ShortcodeId;
use super::tests::Changes;
use super::tests_filtered::holds_by_rule;
use super::tests_ready::{Rng, Tally};
use super::*;
use crate::filter::{Field, Filter, History};
use crate::types::{InputAction, Transition, TransitionField, TransitionSource, SYSTEM_ACTOR};
use roaring::RoaringBitmap;

fn ticket(epic: &str, title: &str, kind: Kind, parent: Option<&str>, body: &str) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(String::from),
        path: None,
        body: body.into(),
    }
}

fn act(actor: &str, source: TransitionSource) -> Act {
    Act { actor: actor.into(), name: None, source, message: None, cause: None }
}

/// The house decision, with an id.
fn decision(id: &str) -> String {
    format!(
        "<m-decision data-field=\"decision\" id=\"{id}\">\n\n<m-question data-field=\"question\">Pick for {id}?</m-question>\n\n\
         <m-option data-field=\"option\" name=\"Yes\" value=\"yes\"></m-option>\n<m-option data-field=\"option\" name=\"No\" value=\"no\"></m-option>\n\n</m-decision>\n"
    )
}

fn status(store: &Store, id: &str) -> Status {
    store.row(id).unwrap().0.status
}

fn own(store: &Store, id: &str) -> Vec<Transition> {
    store.history(id).unwrap().own
}

fn input_state(store: &Store, ticket: &str, input: &str) -> InputState {
    store.inputs(Some(ticket), None, None).unwrap().into_iter().find(|i| i.id == input).unwrap().state
}

/// A history predicate, as the language reads it.
fn pred(text: &str) -> History {
    match Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).groups[0].preds[0].field.clone() {
        Field::History(h) => h,
        other => panic!("{text} is no history predicate: {other:?}"),
    }
}

/// The ids a filter admits, read at `clock`, in the store's order.
fn admitted(store: &Store, text: &str, clock: Clock) -> Vec<String> {
    let lens = Lens::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).with_clock(clock);
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    store.filtered_page(&q, None, None, None).unwrap().tickets.into_iter().map(|t| t.id.into_string()).collect()
}

fn at(clock_now: i64) -> Clock {
    Clock { fixed: Some(clock_now), ..Clock::default() }
}

/// Every write that moves a status or a kind is a transition — who, from
/// where, why — a birth's first status from nothing; the same status again
/// is none; a kind's change and the status it moved are two, the second
/// caused by the first. Durable: a reopen reads the same log.
#[tokio::test]
async fn every_change_of_a_status_or_a_kind_is_a_transition() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.db");
    let store = Store::open(&path).unwrap();
    store.create_by(ticket("LG", "root", Kind::Scope, None, ""), act("ada", TransitionSource::App)).await.unwrap();
    store.create(ticket("LG", "work", Kind::Build, None, "")).await.unwrap();
    let t = own(&store, "LG-1");
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].field, t[0].from.as_deref(), t[0].to.as_str()), (TransitionField::Status, None, "Pending"));
    assert_eq!((t[0].actor.as_str(), t[0].source, t[0].cause), ("ada", TransitionSource::App, None));
    assert_eq!(own(&store, "LG-2")[0].actor, "anonymous", "login off: anonymous, from the api");

    let mut said = Act { message: Some("starting".into()), name: Some("Bob".into()), ..act("idp:bob", TransitionSource::Api) };
    store.set_status_by("LG-2", Status::Coding, said.clone(), false).await.unwrap();
    said.message = None;
    store.set_status_by("LG-2", Status::Coding, said, false).await.unwrap();
    let t = own(&store, "LG-2");
    assert_eq!(t.len(), 2, "the same status again is no transition: {t:?}");
    assert_eq!((t[1].from.as_deref(), t[1].to.as_str()), (Some("Pending"), "Coding"));
    assert_eq!((t[1].actor.as_str(), t[1].name.as_deref(), t[1].message.as_deref()), ("idp:bob", Some("Bob"), Some("starting")));

    store.set_kind_by("LG-2", Kind::Gap, Some(Status::Complete), act("ada", TransitionSource::Pipeline)).await.unwrap();
    let t = own(&store, "LG-2");
    assert_eq!(t.len(), 4);
    assert_eq!((t[2].field, t[2].from.as_deref(), t[2].to.as_str()), (TransitionField::Kind, Some("build"), "gap"));
    assert_eq!((t[3].field, t[3].from.as_deref(), t[3].to.as_str()), (TransitionField::Status, Some("Coding"), "Complete"));
    assert_eq!(t[3].cause, Some(t[2].id), "the status moved because the kind did");
    assert!(t.windows(2).all(|w| w[0].id < w[1].id), "ids in the order written");
    assert!(matches!(store.history("LG-9"), Err(StoreError::NotFound(_))));

    let before = (own(&store, "LG-1"), own(&store, "LG-2"), store.history("LG-1").unwrap().began);
    assert!(before.2.is_some(), "a file's log began when the file gained it");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!((own(&store, "LG-1"), own(&store, "LG-2"), store.history("LG-1").unwrap().began), before, "durable");
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { transitions: Some(5), .. }));
}

/// The tree the cascade tests cancel: R-1 the root; R-2 a parent; under it
/// R-3 (Pending, an open decision) and its child R-4 (Coding, a decision
/// answered); R-5 Complete; R-6 a doc; R-7 canceled on its own; R-8 an
/// open build elsewhere.
async fn tree(store: &Store) {
    store.create(ticket("R", "root", Kind::Scope, None, "")).await.unwrap();
    store.create(ticket("R", "parent", Kind::Build, Some("R-1"), &decision("own"))).await.unwrap();
    store.create(ticket("R", "child", Kind::Build, Some("R-2"), &decision("ask"))).await.unwrap();
    store.create(ticket("R", "grandchild", Kind::Execution, Some("R-3"), &decision("done"))).await.unwrap();
    store.create(ticket("R", "finished", Kind::Build, Some("R-2"), "")).await.unwrap();
    store.create(ticket("R", "a doc", Kind::Doc, Some("R-2"), "")).await.unwrap();
    store.create(ticket("R", "dropped before", Kind::Spike, Some("R-2"), "")).await.unwrap();
    store.create(ticket("R", "elsewhere", Kind::Build, Some("R-1"), "")).await.unwrap();
    store.set_status("R-4", Status::Coding).await.unwrap();
    store.answer("R-4", "done", InputValue::Choice { option: "yes".into() }, None, AnswerSource::Api).await.unwrap();
    store.set_status("R-5", Status::Complete).await.unwrap();
    store.set_status("R-7", Status::Canceled).await.unwrap();
}

/// Canceling a ticket cancels its open descendants in the same write: each
/// a transition of the system's, from `cascade`, naming its cause and who,
/// caused by the cancel; done work, docs and what was canceled already stay;
/// each canceled ticket's open inputs are withdrawn (an answered one
/// stays), out of `inputs { state: open }` and out of `asking`. The write's
/// events: every ticket's change and transition, then its groups, then one
/// `placements_changed`, then the inputs.
#[tokio::test]
async fn canceling_cascades_through_the_open_work_below() {
    let store = Store::new();
    tree(&store).await;
    assert_eq!(admitted(&store, "asking", Clock::default()), ["R-2", "R-3"]);
    let mut rx = store.subscribe();
    let ada = Act { name: Some("Ada".into()), message: Some("folded into R-8".into()), ..act("idp:ada", TransitionSource::App) };
    let answer = store.set_status_by("R-2", Status::Canceled, ada, false).await.unwrap();
    assert!(matches!(&answer, Change::StatusChanged { ticket, previous: Status::Pending } if ticket.id == "R-2"), "{answer:?}");

    for (id, want) in [
        ("R-2", Status::Canceled),
        ("R-3", Status::Canceled),
        ("R-4", Status::Canceled),
        ("R-5", Status::Complete),
        ("R-6", Status::Doc),
        ("R-7", Status::Canceled),
        ("R-8", Status::Pending),
        ("R-1", Status::Pending),
    ] {
        assert_eq!(status(&store, id), want, "{id}");
    }
    let cancel = own(&store, "R-2").pop().unwrap();
    assert_eq!((cancel.actor.as_str(), cancel.source, cancel.message.as_deref()), ("idp:ada", TransitionSource::App, Some("folded into R-8")));
    for (id, from) in [("R-3", "Pending"), ("R-4", "Coding")] {
        let t = own(&store, id).pop().unwrap();
        assert_eq!((t.from.as_deref(), t.to.as_str()), (Some(from), "Canceled"), "{id}");
        assert_eq!((t.actor.as_str(), t.source, t.cause), (SYSTEM_ACTOR, TransitionSource::Cascade, Some(cancel.id)), "{id}");
        assert_eq!(t.message.as_deref(), Some("canceled because R-2 was canceled by Ada"), "{id}");
        assert_eq!(t.cause_ticket.as_deref(), Some("R-2"), "{id}");
    }
    assert_eq!(own(&store, "R-7").len(), 2, "canceled on its own already: untouched");
    let h = store.history("R-2").unwrap();
    assert_eq!(h.caused.iter().map(|t| t.ticket.as_str()).collect::<Vec<_>>(), ["R-3", "R-4"]);
    assert_eq!(h.restorable, ["R-3", "R-4"]);

    // Inputs: the open ones withdrawn, with why; the answered one stays.
    assert_eq!(input_state(&store, "R-2", "own"), InputState::Withdrawn);
    assert_eq!(input_state(&store, "R-3", "ask"), InputState::Withdrawn);
    assert_eq!(input_state(&store, "R-4", "done"), InputState::Answered);
    let ask = store.inputs(Some("R-3"), None, None).unwrap().remove(0);
    let last = ask.history.last().unwrap();
    assert_eq!((last.action, last.source, last.rationale.as_deref()), (InputAction::Withdrawn, AnswerSource::System, Some("canceled because R-2 was canceled by Ada")));
    assert!(store.inputs(None, None, Some(InputState::Open)).unwrap().is_empty(), "nothing waits");
    assert!(admitted(&store, "asking", Clock::default()).is_empty(), "a withdrawn input asks no one");
    assert!(matches!(
        store.answer("R-3", "ask", InputValue::Choice { option: "yes".into() }, None, AnswerSource::Api).await,
        Err(StoreError::InputWithdrawn { .. })
    ));

    // The write's events, in one write's order.
    let mut said = Vec::new();
    while let Ok(c) = rx.try_recv() {
        said.push(c);
    }
    let kind = |c: &Change| match c {
        Change::StatusChanged { ticket, .. } => format!("status {}", ticket.id),
        Change::Transition { transition, .. } => format!("transition {}", transition.ticket),
        Change::GroupChanged { group } => format!("group {}", group.epic),
        Change::PlacementsChanged { .. } => "placements".into(),
        Change::InputChanged { input, .. } => format!("input {}", input.ticket),
        other => format!("{other:?}"),
    };
    let kinds: Vec<String> = said.iter().map(kind).collect();
    assert_eq!(
        kinds[..6],
        ["status R-2", "transition R-2", "status R-3", "transition R-3", "status R-4", "transition R-4"],
        "{kinds:?}"
    );
    assert!(kinds.iter().filter(|k| *k == "placements").count() <= 1, "{kinds:?}");
    let inputs_at = kinds.iter().position(|k| k.starts_with("input")).unwrap();
    assert!(kinds[inputs_at..].iter().all(|k| k.starts_with("input")), "inputs last: {kinds:?}");
    assert_eq!(kinds[inputs_at..], ["input R-2", "input R-3"]);
}

/// Leaving `Canceled` restores the ticket's withdrawn inputs; with
/// `restore_children`, exactly the descendants its cancel canceled and that
/// nothing moved since, to the statuses they had — each the system's,
/// caused by the restore; one canceled on its own, or moved since, stays.
/// Without it, only the ticket and its inputs come back.
#[tokio::test]
async fn restoring_brings_back_exactly_what_the_cancel_canceled() {
    let store = Store::new();
    tree(&store).await;
    store.set_status("R-2", Status::Canceled).await.unwrap();
    store.set_status("R-4", Status::Ready).await.unwrap();
    assert_eq!(store.history("R-2").unwrap().restorable, ["R-3"], "R-4 moved since");
    store.set_status_by("R-2", Status::Ready, act("idp:ada", TransitionSource::Api), true).await.unwrap();
    assert_eq!(status(&store, "R-2"), Status::Ready);
    assert_eq!(status(&store, "R-3"), Status::Pending, "back to what it had");
    assert_eq!(status(&store, "R-4"), Status::Ready, "moved since: as it was moved");
    assert_eq!(status(&store, "R-7"), Status::Canceled, "canceled on its own");
    let restore = own(&store, "R-2").pop().unwrap();
    let back = own(&store, "R-3").pop().unwrap();
    assert_eq!((back.from.as_deref(), back.to.as_str(), back.cause), (Some("Canceled"), "Pending", Some(restore.id)));
    assert_eq!((back.actor.as_str(), back.source), (SYSTEM_ACTOR, TransitionSource::Cascade));
    assert_eq!(back.message.as_deref(), Some("restored to Pending because R-2 was restored by idp:ada"));
    assert_eq!(input_state(&store, "R-2", "own"), InputState::Open);
    assert_eq!(input_state(&store, "R-3", "ask"), InputState::Open);
    assert!(store.history("R-2").unwrap().restorable.is_empty());
    assert_eq!(admitted(&store, "asking", Clock::default()), ["R-2", "R-3"]);

    // Without restore_children: the ticket, its inputs, nothing below.
    store.set_status("R-2", Status::Canceled).await.unwrap();
    store.set_status("R-2", Status::Pending).await.unwrap();
    assert_eq!((status(&store, "R-2"), status(&store, "R-3"), status(&store, "R-4")), (Status::Pending, Status::Canceled, Status::Canceled));
    assert_eq!((input_state(&store, "R-2", "own"), input_state(&store, "R-3", "ask")), (InputState::Open, InputState::Withdrawn));
    assert!(store.history("R-2").unwrap().restorable.is_empty(), "restorable only while canceled");

    // A body written while canceled declares its new inputs withdrawn.
    store.amend("R-3", None, Some(format!("{}{}", decision("ask"), decision("later")))).await.unwrap();
    assert_eq!(input_state(&store, "R-3", "later"), InputState::Withdrawn);
    store.set_status("R-3", Status::Ready).await.unwrap();
    assert_eq!((input_state(&store, "R-3", "ask"), input_state(&store, "R-3", "later")), (InputState::Open, InputState::Open));
}

/// Cascades and restores among random writes: after each, every kept
/// structure is a scan — the counts and the ready set, the cells (`asking`
/// included), the kept tree, the placements, the stored form — and the
/// file's log agrees with its tickets (`--verify-derived`).
#[tokio::test]
async fn cascades_and_restores_keep_every_kept_structure_a_scan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cascade.db");
    let store = Store::open(&path).unwrap();
    let mut rng = Rng(0x4E13_CA5C);
    let mut tally = Tally::asking_and_labelling();
    let epics = ["CA", "CB", "CC"];
    let (mut cancels, mut restores, mut cascaded) = (0usize, 0usize, 0usize);
    for step in 0..260 {
        let what = if step % 4 == 3 {
            let rows = super::tests_ready::rows(&store);
            let work: Vec<&Row> = rows.iter().filter(|t| t.kind.has_lifecycle() && t.child_count > 0).collect();
            let canceled: Vec<&Row> = rows.iter().filter(|t| t.status == Status::Canceled).collect();
            if !canceled.is_empty() && rng.below(2) == 0 {
                let t = canceled[rng.below(canceled.len())];
                let to = [Status::Pending, Status::Ready, Status::Coding][rng.below(3)];
                let all = rng.below(3) != 0;
                store.set_status_by(&t.id, to, Act::default(), all).await.unwrap();
                restores += 1;
                format!("restore {} to {to} (children: {all})", t.id)
            } else if !work.is_empty() {
                let t = work[rng.below(work.len())];
                let before = own(&store, &t.id).last().map(|x| x.id);
                store.set_status(&t.id, Status::Canceled).await.unwrap();
                cancels += 1;
                if let Some(c) = own(&store, &t.id).last().filter(|x| Some(x.id) != before) {
                    cascaded += store.tickets.read().unwrap().log.caused_by(c.id).len();
                }
                format!("cancel {}", t.id)
            } else {
                super::tests_ready::random_write(&store, &mut rng, &epics, &mut tally).await
            }
        } else {
            super::tests_ready::random_write(&store, &mut rng, &epics, &mut tally).await
        };
        let after = format!("step {step}: {what}");
        if step % 5 == 0 || what.starts_with("cancel") || what.starts_with("restore") {
            super::tests_ready::assert_kept_is_a_scan(&store, Some(&path), &after);
            super::tests_cells::assert_cells_are_a_scan(&store, Some(&path), &after);
            super::tests_tree::assert_tree_is_a_scan(&store, Some(&path), &after);
            super::tests_placements::assert_kept_placements_are_the_rule(&store, &after);
            super::tests_stored::assert_stored_is_a_scan(&store, &path, &after);
        }
    }
    assert!(cancels > 10 && restores > 5 && cascaded > 5, "{cancels} cancels, {restores} restores, {cascaded} cascaded");
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { transitions: Some(n), .. } => assert!(n > 100, "{n} transitions"),
        other => panic!("{other:?}"),
    }
    // A reopen reads the same log and the same kept state.
    let before: Vec<Vec<Transition>> = super::tests_ready::rows(&store).iter().map(|t| own(&store, &t.id)).collect();
    drop(store);
    let store = Store::open(&path).unwrap();
    let after: Vec<Vec<Transition>> = super::tests_ready::rows(&store).iter().map(|t| own(&store, &t.id)).collect();
    assert_eq!(before, after);
}

/// `--verify-derived` holds the log to the tickets: a hand-edited status
/// the log does not end at, and a cascade with no cause, are named.
#[tokio::test]
async fn verify_names_a_log_that_disagrees_with_its_tickets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edited.db");
    let store = Store::open(&path).unwrap();
    store.create(ticket("VE", "root", Kind::Scope, None, "")).await.unwrap();
    store.set_status("VE-1", Status::Ready).await.unwrap();
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { transitions: Some(2), .. }));
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO transitions (id, ticket, field, from_value, to_value, at_unix, actor, source) \
         VALUES (3, 'VE-1', 'status', 'Ready', 'Complete', 1, 'system', 'cascade')",
        [],
    )
    .unwrap();
    drop(conn);
    match Store::verify_derived(&path).unwrap() {
        Verdict::Differs(d) => {
            let all = d.join("\n");
            assert!(all.contains("status of VE-1: the log's latest transition (3) ends at Complete; the ticket is Ready"), "{all}");
            assert!(all.contains("transition 3: a cascade's, with no cause"), "{all}");
            assert!(all.contains("transition 3: the system's, with no message"), "{all}");
        }
        other => panic!("{other:?}"),
    }
}

/// The windows, read in a zone: `within`, `today`, `yesterday`, `this week`
/// (from Monday), `since`, `before`, `between` — a date its day, a moment
/// itself — in UTC, in Europe/Berlin (its offset that day), and at a fixed
/// offset; zones by name or offset, and a bad one refused.
#[test]
fn windows_are_read_in_the_zone() {
    // Monday 2026-10-12 10:00:00 UTC.
    let now = 1_791_799_200;
    let day = 86_400;
    let midnight = now - 10 * 3_600;
    let utc = at(now);
    let w = |text: &str| pred(&format!("changed {text}")).window.unwrap();
    let b = |c: &Clock, text: &str| c.bounds(Some(&w(text)));
    assert_eq!(b(&utc, "within 90m"), Some((Some(now - 5_400), None)));
    assert_eq!(b(&utc, "within 2w"), Some((Some(now - 14 * day), None)));
    assert_eq!(b(&utc, "today"), Some((Some(midnight), None)));
    assert_eq!(b(&utc, "yesterday"), Some((Some(midnight - day), Some(midnight))));
    assert_eq!(b(&utc, "this week"), Some((Some(midnight), None)), "a Monday: the week began today");
    assert_eq!(b(&at(now + 3 * day), "this week"), Some((Some(midnight), None)), "Thursday: since Monday");
    assert_eq!(b(&utc, "since 2026-10-10"), Some((Some(midnight - 2 * day), None)));
    assert_eq!(b(&utc, "before 2026-10-10T06:30"), Some((None, Some(midnight - 2 * day + 6 * 3_600 + 1_800))));
    assert_eq!(b(&utc, "between 2026-10-01 and 2026-10-02"), Some((Some(midnight - 11 * day), Some(midnight - 9 * day))), "a date is its whole day");
    assert_eq!(b(&utc, "between 2026-10-05 and 2026-10-01"), None, "an empty window");
    assert_eq!(utc.bounds(None), Some((None, None)));
    // Berlin is UTC+2 in October (CEST): its day starts two hours earlier.
    let berlin = at(now).zoned(Some("Europe/Berlin")).unwrap();
    assert_eq!(b(&berlin, "today"), Some((Some(midnight - 7_200), None)));
    assert_eq!(b(&berlin, "yesterday"), Some((Some(midnight - 7_200 - day), Some(midnight - 7_200))));
    assert_eq!(b(&berlin, "within 1h"), b(&utc, "within 1h"), "within is the same everywhere");
    // 23:30 UTC on the 11th is already the 12th in Kolkata (+05:30).
    let late = midnight - 1_800;
    let kolkata = at(late).zoned(Some("+05:30")).unwrap();
    assert_eq!(b(&kolkata, "today"), Some((Some(midnight - 19_800), None)));
    assert_eq!(b(&at(late), "today"), Some((Some(midnight - day), None)));
    for name in ["UTC", "z", "+02:00", "-0530", "+2", "America/New_York", ""] {
        assert!(Clock::default().zoned(Some(name)).is_ok(), "{name}");
    }
    for name in ["Mars/Olympus", "+25:00", "+ab"] {
        assert!(Clock::default().zoned(Some(name)).is_err(), "{name}");
    }
}

/// The kept indexes answer as a scan of the transitions: random logs over a
/// few tickets, actors and hours; every predicate of every event, window
/// and actor, read at random moments, is exactly the tickets whose own
/// transitions the rule passes.
#[test]
fn the_kept_indexes_answer_as_a_scan() {
    let mut rng = Rng(0x10_6113);
    let base = 1_791_799_200;
    let actors = ["anonymous", "system", "idp:ada", "key:bob"];
    let statuses = [Status::Pending, Status::Ready, Status::Coding, Status::InReview, Status::Complete, Status::Canceled];
    for round in 0..8 {
        let mut log = Log::began_at(Some(base - 30 * 86_400));
        let mut by_ticket: BTreeMap<(String, u32), Vec<Transition>> = BTreeMap::new();
        let mut now_of: BTreeMap<(String, u32), Status> = BTreeMap::new();
        for id in 1..=300u64 {
            let key = (["A", "B"][rng.below(2)].to_string(), 1 + rng.below(12) as u32);
            let kind = rng.below(6) == 0;
            let from = now_of.get(&key).copied();
            let to = statuses[rng.below(statuses.len())];
            // Mostly forward in time; now and then a clock that stepped back.
            let at_unix = base - 20 * 86_400 + (id as i64) * 4_000 - if rng.below(10) == 0 { 50_000 } else { 0 };
            let t = Transition {
                id,
                ticket: format!("{}-{}", key.0, key.1).into(),
                field: if kind { TransitionField::Kind } else { TransitionField::Status },
                from: if kind { Some("build".into()) } else { from.map(|s| s.as_str().into()) },
                to: if kind { "gap".into() } else { to.as_str().into() },
                at_unix,
                actor: actors[rng.below(actors.len())].into(),
                name: None,
                source: TransitionSource::Api,
                message: None,
                cause: None,
                cause_ticket: None,
            };
            if !kind {
                now_of.insert(key.clone(), to);
            }
            log.append(&t, &OrderKey { shortcode: ShortcodeId::from(key.0.clone()), seq: key.1 });
            by_ticket.entry(key).or_default().push(t);
        }
        let windows = ["", " within 3h", " within 2d", " today", " yesterday", " this week", " since 2026-10-01", " before 2026-09-30T12:00", " between 2026-09-25 and 2026-10-03"];
        let bys = ["", " by system", " by idp:ada", " by me", " by nobody"];
        for e in ["entered Canceled", "entered Ready", "left Pending", "left \"In Review\"", "changed"] {
            for w in windows {
                for by in bys {
                    let p = pred(&format!("{e}{w}{by}"));
                    for zone in ["UTC", "Europe/Berlin"] {
                        let clock = Clock { me: "key:bob".into(), ..at(base + rng.below(5 * 86_400) as i64 - 86_400) }.zoned(Some(zone)).unwrap();
                        let got = log.holds(&p, &clock);
                        let mut want: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
                        for ((epic, seq), own) in &by_ticket {
                            if holds_by_rule(&p, own, &clock) {
                                want.entry(ShortcodeId::from(epic)).or_default().insert(*seq);
                            }
                        }
                        assert_eq!(got, want, "round {round}: `{}` at {:?} in {zone}", p.format(), clock.fixed);
                    }
                }
            }
        }
    }
}

/// FILTER-3's examples over a real store, read at chosen moments: `status
/// "In Review" and entered "In Review" within 12h`, `entered Coding
/// yesterday`, `changed within 1h and under …`, `entered Complete this week
/// and kind build`, `entered Canceled within 1d by system`, `by me` — and a
/// window that ages out as the clock moves.
#[tokio::test]
async fn the_examples_read_the_log() {
    let store = Store::new();
    tree(&store).await;
    store.set_status("R-8", Status::InReview).await.unwrap();
    store.set_status("R-3", Status::Coding).await.unwrap();
    store.set_status("R-3", Status::InReview).await.unwrap();
    store.set_status("R-3", Status::Ready).await.unwrap();
    store.set_status_by("R-2", Status::Canceled, act("idp:ada", TransitionSource::App), false).await.unwrap();
    let now = chrono::Utc::now().timestamp();
    let clock = at(now);
    assert_eq!(admitted(&store, "status \"In Review\" and entered \"In Review\" within 12h", clock.clone()), ["R-8"]);
    assert_eq!(admitted(&store, "entered \"In Review\" within 12h", clock.clone()), ["R-3", "R-8"]);
    assert_eq!(admitted(&store, "entered Canceled within 1d by system", clock.clone()), ["R-3", "R-4"]);
    assert_eq!(admitted(&store, "entered Canceled by idp:ada", clock.clone()), ["R-2"]);
    assert_eq!(admitted(&store, "changed within 1h and under R-3", clock.clone()), ["R-3", "R-4"]);
    assert_eq!(admitted(&store, "entered Complete this week and kind build", clock.clone()), ["R-5"]);
    assert_eq!(admitted(&store, "not changed within 1h", clock.clone()), Vec::<String>::new(), "every ticket was born within the hour");
    let me = Clock { me: "idp:ada".into(), ..clock.clone() };
    assert_eq!(admitted(&store, "changed by me", me), ["R-2"]);
    // A day on, it entered Coding yesterday; the hour-long windows are empty.
    let tomorrow = at(now + 86_400);
    assert_eq!(admitted(&store, "entered Coding yesterday", tomorrow.clone()), ["R-3", "R-4"]);
    assert!(admitted(&store, "changed within 1h", tomorrow).is_empty());
    // `aged`: what a watch says as the clock passes the hour.
    let lens = Lens::parse("entered \"In Review\" within 1h").unwrap();
    let aged = store.aged(&lens, now, now + 2 * 3_600, None);
    assert_eq!(
        aged,
        [
            Membership::Left { id: "R-3".into(), shortcode: ShortcodeId::from("R"), seq: 3 },
            Membership::Left { id: "R-8".into(), shortcode: ShortcodeId::from("R"), seq: 8 }
        ]
    );
    assert!(store.aged(&lens, now, now + 60, None).is_empty(), "nothing aged in a minute");
    assert!(store.aged(&Lens::parse("entered Canceled since 2026-01-01").unwrap(), now, now + 9 * 86_400, None).is_empty(), "a fixed window does not age");
    // The tree under a history filter: R-2 holds the cascade's two.
    let q = TreeQuery { parent: Some("R-1".into()), epic: None, lens: Lens::parse("entered Canceled within 1d by system").unwrap(), matches_only: false, keep: None };
    let page = store.tree_page(&q, None, None).unwrap();
    assert_eq!(page.nodes.iter().map(|n| (n.summary.id.as_str(), n.matched, n.below)).collect::<Vec<_>>(), [("R-2", false, 2)]);
}

/// The filters through `watch`: a write's transition moves membership as
/// it is made (`entered`), and — FILTER-24 — `keep` holds as with any
/// filter.
#[tokio::test]
async fn a_history_watch_hears_each_writes_transition() {
    let store = Store::new();
    tree(&store).await;
    let lens = Lens::parse("entered Coding within 1h").unwrap();
    let mut watcher = store.watcher(lens);
    let mut rx = store.subscribe_filtered();
    store.set_status("R-8", Status::Coding).await.unwrap();
    let mut heard = Vec::new();
    while let Ok(c) = rx.try_change() {
        if let Change::UnderMoved { moves } = c {
            heard.extend(watcher.under_membership(&moves));
        }
    }
    assert_eq!(heard, [Membership::Entered { id: "R-8".into(), shortcode: ShortcodeId::from("R"), seq: 8 }]);
    assert!(watcher.tick(&store, None).is_empty(), "no time has passed that moves it");
    let kept = store.counts_keeping(&Lens::parse("entered Coding within 1h").unwrap(), Some("R-1"));
    assert_eq!(kept, [(ShortcodeId::from("R"), 3)], "R-4, R-8 and the kept R-1");
}
