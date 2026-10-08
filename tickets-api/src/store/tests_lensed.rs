//! FILTER-15, acceptance 1: every read that takes the lens equals the
//! oracle — FILTER-10's [`Filter::matches`] over every row. On stores built
//! by create, by seed and from the fixtures, after each of hundreds of
//! random writes of every kind and after a reopen, random filters from the
//! language's own AST (every accepted field, `in`, nested `and` / `or` /
//! `not`, lists, a display part):
//!
//! - `find { query, filter }` — common, rare, absent and blank queries,
//!   after random cursors, at random limits: rows, `next`, `remaining`, no
//!   `first`;
//! - `ready_set { epic?, filter }` — MNG-4's rule written out (Ready, every
//!   blocker done with) ∧ the filter, paged the same way;
//! - `member { ids, filter }` — ids that are tickets, lower-cased ones,
//!   ones that are none;
//! - `inputs { …, filter }` — `inputs` without it, kept where the filter
//!   admits the input's ticket.
//!
//! Then what each is without a filter (today's path, untouched), the
//! refusals, path addresses and hand-edited ids.

use super::cells::Key;
use super::tests_filtered::Plain;
use super::tests_language::{filter_names, random_filter};
use super::tests_ready::{new_ticket, random_write, Rng, Tally};
use super::*;
use crate::filter::Filter;

/// One page: the ids, `next`, `remaining`, `first`.
type Answer = (Vec<String>, Option<String>, u64, Option<u64>);

fn answer(page: Page) -> Answer {
    (page.tickets.into_iter().map(|t| t.id).collect(), page.next, page.remaining, page.first)
}

/// The palette's rule, written out: the trimmed query, ASCII case folded, a
/// substring of the id or the title; blank matches every ticket.
fn text_match(t: &Row, query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    q.is_empty() || t.id.to_ascii_lowercase().contains(&q) || t.title.to_ascii_lowercase().contains(&q)
}

/// MNG-4's ready set, by its own rule — `Ready`, and no blocker that is not
/// done with — read off the scan's cell (its `blocked` is that rule's).
fn ready(t: &Row, k: Key) -> bool {
    t.status == Status::Ready && !k.blocked()
}

/// The rows (indices, in the store's order) a judge passes.
fn matched(plain: &Plain, judge: impl Fn(&Row, Key) -> bool) -> Vec<usize> {
    (0..plain.rows.len()).filter(|i| judge(&plain.rows[*i], plain.cells[*i])).collect()
}

/// A page of `matched` by MNG-9's rule: strictly after `after` (a position,
/// upper-cased; one naming no ticket is the cursor's error), at most
/// `limit`; `remaining` what is past it; `next` its last id, or — empty with
/// matches past it — the cursor; no `first`.
fn cut(plain: &Plain, matched: &[usize], after: Option<&str>, limit: Option<u32>) -> Result<Answer, String> {
    let after = after.map(str::to_ascii_uppercase);
    let from = match after.as_deref() {
        None => 0,
        Some(id) => {
            let Some(c) = plain.rows.iter().position(|t| t.id == id) else {
                return Err(format!("after: no ticket {id}"));
            };
            let key = OrderKey::of(&plain.rows[c]);
            matched.iter().filter(|i| OrderKey::of(&plain.rows[**i]) <= key).count()
        }
    };
    let to = limit.map_or(matched.len(), |n| (from + n as usize).min(matched.len()));
    let page: Vec<String> = matched[from..to].iter().map(|i| plain.rows[*i].id.clone()).collect();
    let remaining = (matched.len() - to) as u64;
    let next = if remaining == 0 { None } else { page.last().cloned().or(after) };
    Ok((page, next, remaining, None))
}

/// A query the palette might send: part of a title or an id, a rare one, an
/// absent one, a blank one, in any case.
fn random_query(rng: &mut Rng, plain: &Plain) -> String {
    let pick = |rng: &mut Rng| &plain.rows[rng.below(plain.rows.len())];
    let part = |rng: &mut Rng, s: &str| -> String {
        let chars: Vec<char> = s.chars().collect();
        if chars.is_empty() {
            return String::new();
        }
        let from = rng.below(chars.len());
        let len = 1 + rng.below((chars.len() - from).min(6));
        chars[from..from + len].iter().collect()
    };
    let q = match (plain.rows.is_empty(), rng.below(9)) {
        (true, _) | (_, 0) => "zz-nothing-has-this".to_string(),
        (_, 1) => "   ".to_string(),
        (_, 2) => String::new(),
        (_, 3 | 4) => {
            let t = pick(rng).title.clone();
            part(rng, &t)
        }
        (_, 5 | 6) => {
            let id = pick(rng).id.clone();
            part(rng, &id)
        }
        (_, 7) => pick(rng).id.clone(),
        _ => format!(" {} ", pick(rng).title),
    };
    match rng.below(3) {
        0 => q.to_ascii_uppercase(),
        1 => q.to_ascii_lowercase(),
        _ => q,
    }
}

/// A cursor: an id of the store (in its own case, or lower-cased), or none.
fn random_after(rng: &mut Rng, plain: &Plain) -> Option<String> {
    match (plain.rows.is_empty(), rng.below(4)) {
        (true, _) | (_, 0) => None,
        (_, 1) => Some(plain.rows[rng.below(plain.rows.len())].id.to_ascii_lowercase()),
        _ => Some(plain.rows[rng.below(plain.rows.len())].id.clone()),
    }
}

fn random_limit(rng: &mut Rng) -> Option<u32> {
    match rng.below(6) {
        0 => None,
        1 => Some(0),
        2 => Some(1),
        _ => Some(1 + rng.below(40) as u32),
    }
}

/// How much the random reads exercised: answers that held rows, and ones
/// that held none — so a walk cannot pass by asking only empty questions.
#[derive(Debug, Default)]
struct Seen {
    find_rows: usize,
    find_empty: usize,
    ready_rows: usize,
    member_true: usize,
    member_false: usize,
}

/// `find { query, filter }`: a page after random cursors, equal to the
/// oracle's; then a walk by `next` that visits every match once.
#[track_caller]
fn assert_find_is_a_scan(store: &Store, plain: &Plain, filter: &Filter, rng: &mut Rng, seen: &mut Seen, what: &str) {
    let lens = Lens::of(filter).unwrap();
    let judge = |t: &Row, k: Key| filter.matches(&plain.facts(t, k));
    for _ in 0..6 {
        let query = random_query(rng, plain);
        let m = matched(plain, |t, k| text_match(t, &query) && judge(t, k));
        let (after, limit) = (random_after(rng, plain), random_limit(rng));
        let got = store.find_filtered(&query, &lens, after.as_deref(), limit).map(answer).map_err(|e| e.to_string());
        let want = cut(plain, &m, after.as_deref(), limit);
        match &want {
            Ok(w) if !w.0.is_empty() => seen.find_rows += 1,
            _ => seen.find_empty += 1,
        }
        match (&got, &want) {
            (Ok(g), Ok(w)) => assert_eq!(g, w, "{what}: find {query:?} under `{}` after {after:?} limit {limit:?}", filter.format()),
            (Err(_), Err(_)) => {}
            _ => panic!("{what}: find {query:?} under `{}` after {after:?}: {got:?} vs {want:?}", filter.format()),
        }
    }
    // Every match once, a page at a time.
    let query = random_query(rng, plain);
    let m = matched(plain, |t, k| text_match(t, &query) && judge(t, k));
    let (mut after, mut seen) = (None::<String>, Vec::new());
    loop {
        let page = store.find_filtered(&query, &lens, after.as_deref(), Some(7)).unwrap();
        assert_eq!(page.first, None, "{what}: find gives no `first`");
        seen.extend(page.tickets.iter().map(|t| t.id.clone()));
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    let want: Vec<String> = m.iter().map(|i| plain.rows[*i].id.clone()).collect();
    if want.iter().all(|id| *id == id.to_ascii_uppercase()) {
        assert_eq!(seen, want, "{what}: a walk of find {query:?} under `{}`", filter.format());
    }
}

/// `ready_set { epic?, filter }`: pages after random cursors, in the whole
/// store or one shortcode, equal to MNG-4's rule ∧ the filter.
#[track_caller]
fn assert_ready_is_a_scan(store: &Store, plain: &Plain, filter: &Filter, epics: &[String], rng: &mut Rng, seen: &mut Seen, what: &str) {
    for _ in 0..5 {
        let epic = match rng.below(3) {
            0 => Some(epics[rng.below(epics.len())].clone()),
            1 => Some(epics[rng.below(epics.len())].to_ascii_lowercase()),
            _ => None,
        };
        let m = matched(plain, |t, k| {
            ready(t, k) && filter.matches(&plain.facts(t, k)) && epic.as_deref().is_none_or(|e| t.epic == e.to_ascii_uppercase())
        });
        let (after, limit) = (random_after(rng, plain), random_limit(rng));
        let q = ready_and(filter, epic.clone()).unwrap();
        let got = store.ready_filtered_page(&q, after.as_deref(), limit).map(answer).map_err(|e| e.to_string());
        let want = cut(plain, &m, after.as_deref(), limit);
        seen.ready_rows += usize::from(want.as_ref().is_ok_and(|w| !w.0.is_empty()));
        match (&got, &want) {
            (Ok(g), Ok(w)) => assert_eq!(g, w, "{what}: ready_set in {epic:?} under `{}` after {after:?} limit {limit:?}", filter.format()),
            (Err(_), Err(_)) => {}
            _ => panic!("{what}: ready_set under `{}` after {after:?}: {got:?} vs {want:?}", filter.format()),
        }
    }
}

/// `member { ids, filter }`: tickets in their case and lower-cased, and ids
/// that name none — in the order given.
#[track_caller]
fn assert_member_is_a_scan(store: &Store, plain: &Plain, filter: &Filter, rng: &mut Rng, seen: &mut Seen, what: &str) {
    let mut ids: Vec<String> = Vec::new();
    for _ in 0..40 {
        ids.push(match (plain.rows.is_empty(), rng.below(6)) {
            (true, _) | (_, 0) => format!("NONE-{}", rng.below(9)),
            (_, 1) => plain.rows[rng.below(plain.rows.len())].id.to_ascii_lowercase(),
            (_, 2) => format!(" {} ", plain.rows[rng.below(plain.rows.len())].id),
            _ => plain.rows[rng.below(plain.rows.len())].id.clone(),
        });
    }
    let want: Vec<bool> = ids
        .iter()
        .map(|id| {
            let id = id.trim();
            let at = plain.rows.iter().position(|t| t.id == id).or_else(|| plain.rows.iter().position(|t| t.id == id.to_ascii_uppercase()));
            at.is_some_and(|i| filter.matches(&plain.facts(&plain.rows[i], plain.cells[i])))
        })
        .collect();
    seen.member_true += want.iter().filter(|m| **m).count();
    seen.member_false += want.iter().filter(|m| !**m).count();
    assert_eq!(store.member(&ids, &Lens::of(filter).unwrap()), want, "{what}: member {ids:?} under `{}`", filter.format());
}

/// Every read under one random filter.
fn assert_reads_are_a_scan(store: &Store, plain: &Plain, rng: &mut Rng, epics: &[String], seen: &mut Seen, what: &str) {
    let filter = random_filter(rng, epics);
    assert_find_is_a_scan(store, plain, &filter, rng, seen, what);
    assert_ready_is_a_scan(store, plain, &filter, epics, rng, seen, what);
    assert_member_is_a_scan(store, plain, &filter, rng, seen, what);
}

/// The shortcodes a store holds, one it does not, and ticket ids —
/// FILTER-13: what the random filters' `in` and `under` name.
fn names(store: &Store) -> Vec<String> {
    filter_names(store)
}

async fn walk(store: Store, path: &Path, seed: u64, writes: usize, every: usize, epics: &[&str]) {
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    let mut seen = Seen::default();
    for step in 0..writes {
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        if step % every == every - 1 {
            let plain = Plain::of(&store);
            let held = names(&store);
            for _ in 0..3 {
                assert_reads_are_a_scan(&store, &plain, &mut rng, &held, &mut seen, &format!("step {step}: {what}"));
            }
        }
    }
    drop(store);
    let store = Store::open(path).unwrap();
    let (plain, held) = (Plain::of(&store), names(&store));
    for _ in 0..8 {
        assert_reads_are_a_scan(&store, &plain, &mut rng, &held, &mut seen, "the reopen");
    }
    assert!(
        seen.find_rows > 10 && seen.find_empty > 10 && seen.ready_rows > 3 && seen.member_true > 50 && seen.member_false > 50,
        "the walk asked questions with answers and without: {seen:?}"
    );
}

#[tokio::test]
async fn reads_under_random_filters_equal_the_evaluator_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    walk(store, &path, 0xF115_C0DE, 450, 9, &["AA", "MAP", "SUB", "ZED"]).await;
}

#[tokio::test]
async fn reads_under_random_filters_equal_the_evaluator_on_a_seeded_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seeded.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 3_000, seed: 415, into_existing: false }).unwrap();
    let epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).take(5).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    walk(store, &path, 0x5EED_F415, 200, 25, &epics).await;
}

#[tokio::test]
async fn reads_under_random_filters_equal_the_evaluator_on_each_fixture() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        let store = Store::open(&path).unwrap();
        let mut epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        walk(store, &path, 0xF415 + i as u64, 120, 12, &epics).await;
    }
}

// ---------------------------------------------------------------------------
// inputs { filter }

fn decision(id: &str) -> String {
    format!(
        "<m-decision data-field=\"decision\" id=\"{id}\">\n<m-question data-field=\"question\">Pick?</m-question>\n\
         <m-option data-field=\"option\" name=\"A\" value=\"a\"></m-option>\n<m-option data-field=\"option\" name=\"B\" value=\"b\"></m-option>\n\
         </m-decision>\n"
    )
}

/// `inputs` under random filters equals `inputs` kept where the filter
/// admits the input's ticket — with `ticket`, `epic` and `state` beside it —
/// after random writes of every kind (which move tickets in and out of
/// filters), answers, closes, and a body that drops its input.
#[tokio::test]
async fn inputs_under_random_filters_are_the_inputs_of_the_tickets_the_filter_admits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inputs.db");
    let store = Store::open(&path).unwrap();
    let epics = ["IN", "OUT", "ZED"];
    for e in epics {
        store.create(new_ticket(e, Kind::Scope, None, vec![])).await.unwrap();
    }
    let mut rng = Rng(0x1_F415);
    let mut tally = Tally::default();
    let mut declared: Vec<String> = Vec::new();
    for step in 0..240 {
        let what = match rng.below(5) {
            0 => {
                let epic = epics[rng.below(epics.len())];
                let mut t = new_ticket(epic, Kind::Build, None, vec![]);
                t.body = format!("{}{}", decision("pick"), if rng.below(2) == 0 { decision("second") } else { String::new() });
                let id = store.create(t).await.unwrap().id;
                declared.push(id.clone());
                format!("create {id} with inputs")
            }
            1 if !declared.is_empty() => {
                let id = declared[rng.below(declared.len())].clone();
                let _ = store.answer(&id, "pick", InputValue::Choice { option: "b".into() }, None, AnswerSource::Api).await;
                if rng.below(3) == 0 {
                    let _ = store.close_input(&id, "pick").await;
                }
                format!("answer {id}")
            }
            2 if !declared.is_empty() && rng.below(4) == 0 => {
                let id = declared[rng.below(declared.len())].clone();
                store.amend(&id, None, Some("no inputs now".into())).await.unwrap();
                format!("amend {id} without its inputs")
            }
            _ => random_write(&store, &mut rng, &epics, &mut tally).await,
        };
        if step % 8 != 7 {
            continue;
        }
        let plain = Plain::of(&store);
        let held = names(&store);
        for _ in 0..4 {
            let filter = random_filter(&mut rng, &held);
            let lens = Lens::of(&filter).unwrap();
            let ticket = match rng.below(4) {
                0 if !declared.is_empty() => Some(declared[rng.below(declared.len())].to_ascii_lowercase()),
                _ => None,
            };
            let epic = (rng.below(3) == 0).then(|| epics[rng.below(epics.len())].to_ascii_lowercase());
            let state = match rng.below(4) {
                0 => Some(InputState::Open),
                1 => Some(InputState::Answered),
                2 => Some(InputState::Closed),
                _ => None,
            };
            let admitted = |id: &str| {
                let i = plain.rows.iter().position(|t| t.id == id).expect("an input's ticket is a ticket");
                filter.matches(&plain.facts(&plain.rows[i], plain.cells[i]))
            };
            let want: Vec<Input> = store
                .inputs(ticket.as_deref(), epic.as_deref(), state)
                .unwrap()
                .into_iter()
                .filter(|i| admitted(&i.ticket))
                .collect();
            let got = store.inputs_filtered(ticket.as_deref(), epic.as_deref(), state, &lens).unwrap();
            assert_eq!(got, want, "step {step} ({what}): inputs {ticket:?} {epic:?} {state:?} under `{}`", filter.format());
        }
    }
    assert!(store.inputs(None, None, None).unwrap().len() > 20, "enough inputs to filter");
    assert!(matches!(store.inputs_filtered(Some("NONE-1"), None, None, &Lens::all()), Err(StoreError::NotFound(_))));
}

// ---------------------------------------------------------------------------
// Without a filter, and the edges.

/// A filter that narrows nothing — none, empty, blank, a display part
/// alone — is no filter: the read takes today's path. One the language
/// does not take, or that asks `text`, is FILTER-4's refusal, word for
/// word. FILTER-13: one that asks `under` is taken.
#[test]
fn a_filter_that_narrows_nothing_is_no_filter_and_a_refusal_is_filter_4s() {
    for text in [None, Some(""), Some("   "), Some("· flat"), Some("· columns id title · order code")] {
        assert!(narrowing(text).unwrap().is_none(), "{text:?}");
    }
    assert_eq!(narrowing(Some("kind build")).unwrap().unwrap().format(), "kind build");
    assert_eq!(narrowing(Some("under SCALE-13 and ready")).unwrap().unwrap().format(), "under SCALE-13 and ready");
    for text in ["text \"watch\"", "under SCALE-13 and text \"x\"", "tag x and nonsense", "status Bogus", "board by status"] {
        let refused = narrowing(Some(text)).unwrap_err().to_string();
        assert_eq!(refused, Lens::parse(text).unwrap_err().to_string(), "{text}");
    }
    assert!(narrowing(Some("text \"watch\"")).unwrap_err().to_string().contains("text is the palette's"));
}

/// A filter that admits every ticket answers `find` and `ready_set` exactly
/// as no filter does — rows, `next`, `remaining` — page for page.
#[tokio::test]
async fn a_filter_admitting_everything_answers_as_no_filter() {
    let store = Store::new();
    store.seed_synthetic(SeedSpec { count: 2_500, seed: 15, into_existing: false }).unwrap();
    let all = Filter::parse("(status Ready or not status Ready)").unwrap();
    let lens = Lens::of(&all).unwrap();
    let plain = Plain::of(&store);
    let mut rng = Rng(0xA11);
    for _ in 0..60 {
        let query = random_query(&mut rng, &plain);
        let (after, limit) = (random_after(&mut rng, &plain), random_limit(&mut rng));
        let filtered = store.find_filtered(&query, &lens, after.as_deref(), limit).map(answer).map_err(|e| e.to_string());
        let plain_find = store.find_page(&query, after.as_deref(), limit).map(answer).map_err(|e| e.to_string());
        assert_eq!(filtered, plain_find, "find {query:?} after {after:?} limit {limit:?}");
        let epic = (rng.below(2) == 0).then(|| plain.rows[rng.below(plain.rows.len())].epic.clone());
        let q = ready_and(&all, epic.clone()).unwrap();
        let filtered = store.ready_filtered_page(&q, after.as_deref(), limit).map(answer).map_err(|e| e.to_string());
        let ready = store.ready_page(epic.as_deref(), after.as_deref(), limit).map(answer).map_err(|e| e.to_string());
        assert_eq!(filtered, ready, "ready_set in {epic:?} after {after:?} limit {limit:?}");
    }
}

/// `find`'s cursor is a position, as without a filter: one naming no ticket
/// is the cursor's error; one that does not match starts after it.
#[tokio::test]
async fn find_under_a_filter_pages_by_position() {
    let store = Store::new();
    for (epic, kind) in [("A", Kind::Scope), ("A", Kind::Build), ("A", Kind::Build), ("B", Kind::Scope), ("B", Kind::Build)] {
        store.create(new_ticket(epic, kind, None, vec![])).await.unwrap();
    }
    let lens = Lens::parse("kind build").unwrap();
    let ids = |p: &Page| p.tickets.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
    let page = store.find_filtered("a", &lens, None, Some(2)).unwrap();
    assert_eq!((ids(&page), page.next.as_deref(), page.remaining, page.first), (vec!["A-2".into(), "A-3".into()], Some("A-3"), 1, None));
    let page = store.find_filtered("a", &lens, Some("b-1"), None).unwrap();
    assert_eq!((ids(&page), page.next, page.remaining), (vec!["B-2".to_string()], None, 0), "after a ticket the filter does not admit");
    let page = store.find_filtered("a", &lens, Some("A-1"), Some(0)).unwrap();
    assert_eq!((ids(&page).len(), page.next.as_deref(), page.remaining), (0, Some("A-1"), 3), "an empty page hands its cursor back");
    assert!(matches!(store.find_filtered("a", &lens, Some("C-1"), None), Err(StoreError::CursorNotFound(_))));
    let page = store.find_filtered("", &lens, None, None).unwrap();
    assert_eq!((ids(&page).len(), page.remaining, page.first), (3, 0, None), "a blank query: every ticket the filter admits");
    let page = store.find_filtered("scope", &lens, None, None).unwrap();
    assert!(page.tickets.is_empty() && page.next.is_none(), "text matches the filter does not admit");
}

/// `member` names a ticket as `get` does — by id (in the case it has, or
/// upper-cased) or by path address — and an id naming none is not admitted;
/// answers come in the order asked, repeats included.
#[tokio::test]
async fn member_names_a_ticket_as_get_does() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("member.db");
    super::tests_paging::write_runs(&path, &[("low", 1), ("low", 2), ("UP", 1)]);
    Connection::open(&path)
        .unwrap()
        .execute_batch("UPDATE tickets SET status = 'Ready' WHERE seq = 1; UPDATE tickets SET kind = 'scope';")
        .unwrap();
    let store = Store::open(&path).unwrap();
    let mut board = new_ticket("UP", Kind::Doc, None, vec![]);
    board.path = Some("_MENGER/board".into());
    assert_eq!(store.create(board).await.unwrap().id, "UP-2");
    let ready = Lens::parse("status Ready").unwrap();
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let asked = ids(&["low-1", "LOW-1", "low-2", "up-1", "UP-1", " UP-1 ", "NOPE-1", "", "UP-1"]);
    assert_eq!(store.member(&asked, &ready), [true, false, false, true, true, true, false, false, true], "{asked:?}");
    let docs = Lens::parse("kind doc").unwrap();
    let asked = ids(&["UP/_MENGER/board", "up/_MENGER/board", "UP/_MENGER/none", "UP-2", "UP-1"]);
    assert_eq!(store.member(&asked, &docs), [true, true, false, true, false], "{asked:?}");
    assert_eq!(store.member(&ids(&["low-1"]), &Lens::parse("in LOW").unwrap()), [true], "`in` reads a shortcode whatever its case");
    assert!(store.member(&[], &ready).is_empty());
}

/// Not a test: the store's own time for the reads that take the lens, at
/// the size of `READS_DB` (a copy of it is opened — e.g. the commit
/// before's `--seed-synthetic 1000000`), FILTER-3 §6.2's nine filters,
/// medians of 21 — without the transport, whose cost per event
/// `perf/reads-probe.mjs` adds.
/// `READS_DB=… cargo test --release -p menger-tickets-api --lib reads_at_a_million -- --ignored --nocapture`
#[test]
#[ignore]
fn reads_at_a_million() {
    let Ok(source) = std::env::var("READS_DB") else {
        eprintln!("READS_DB names a file to read");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reads.db");
    std::fs::copy(source, &path).unwrap();
    let store = Store::open(&path).unwrap();
    let median = |f: &mut dyn FnMut()| -> f64 {
        let mut ts: Vec<f64> = (0..21)
            .map(|_| {
                let t = std::time::Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        ts.sort_by(f64::total_cmp);
        ts[10]
    };
    let extents = store.extents();
    let total: u64 = extents.iter().map(|(_, n)| u64::from(*n)).sum();
    let mut rng = Rng(0x15);
    let ids: Vec<String> = (0..MEMBER_MAX)
        .map(|_| {
            let mut k = rng.below(total as usize) as u64;
            for (e, n) in &extents {
                if k < u64::from(*n) {
                    return format!("{e}-{}", k + 1);
                }
                k -= u64::from(*n);
            }
            unreachable!()
        })
        .collect();
    let queries = ["audit", "zzqx", "the"];
    let mut unfiltered = Vec::new();
    for q in queries {
        let ms = median(&mut || drop(store.find_page(q, None, Some(50)).unwrap()));
        unfiltered.push(ms);
        println!("find {q:?}: {ms:.3} ms");
    }
    for text in [
        "status Ready",
        "status in (Pending, Ready) and kind build",
        "ready",
        "blocked and severity critical",
        "kind scope and status Idea and severity critical",
        "category in (done, dropped)",
        "(status Blocked or blocked) and not kind doc",
        "not kind doc",
        "category in (active, review)",
    ] {
        let lens = Lens::parse(text).unwrap();
        let admits: u64 = store.counts(&lens).iter().map(|(_, n)| n).sum();
        let finds: Vec<String> = queries
            .iter()
            .zip(&unfiltered)
            .map(|(q, base)| {
                let ms = median(&mut || drop(store.find_filtered(q, &lens, None, Some(50)).unwrap()));
                format!("{q:?} {ms:.3} ms ({:.3}×)", ms / base)
            })
            .collect();
        let member = median(&mut || drop(store.member(&ids, &lens)));
        let q = ready_and(&Filter::parse(text).unwrap(), None).unwrap();
        let ready = median(&mut || drop(store.ready_filtered_page(&q, None, Some(200)).unwrap()));
        println!(
            "`{text}` admits {admits} ({:.2} %): find {}; member of {} ids {member:.3} ms; ready_set page of 200 {ready:.3} ms",
            100.0 * admits as f64 / total as f64,
            finds.join(", "),
            ids.len()
        );
    }
}
