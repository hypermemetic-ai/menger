//! SCALE-32: what `ready_set` and `list { status }` answer from — each
//! ticket's count of unsatisfied blockers, the ready set, each shortcode's
//! count of each status — held to a scan after every write, in the cache
//! and read back from the file through a connection of its own; and every
//! page of the two, rows and `remaining`, held to the page the code before
//! it gave — that code kept here, word for word, as the oracle, beside
//! MNG-4's and MNG-9's rules applied to the store's order as a plain list.
//!
//! The stores are built every way tickets reach the cache: by `create`, by
//! a seed (fresh and into an existing store), from each fixture written
//! before SCALE-28, from a SCALE-28 file (form 1) and from hand-edited
//! shapes; and the writes are every kind, with what moves membership
//! weighted up — births with blockers that exist, that are named before
//! they are born, that never will be and that are named twice; status
//! changes into and out of `Complete` and `Superseded`; re-parents.

use super::stored::{self, State};
use super::*;
use std::collections::BTreeSet;

/// SplitMix64, as the store's other randomized tests use: reproducible,
/// dependency-free.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n as u64) as usize
    }
}

pub(super) fn new_ticket(epic: &str, kind: Kind, parent: Option<String>, blocked_by: Vec<String>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by,
        parent,
        body: String::new(),
    }
}

/// A blocker that is done with — written out, not [`satisfied`]: the oracle
/// owes the code under test nothing.
pub(super) fn done(status: Status) -> bool {
    matches!(status, Status::Complete | Status::Superseded | Status::Canceled)
}

/// Every row of the store, in its order.
pub(super) fn rows(store: &Store) -> Vec<Row> {
    store.tickets.read().unwrap().order.values().cloned().collect()
}

/// What a scan of the tickets gives — the oracle for the kept state.
#[derive(Debug, PartialEq)]
struct Scan {
    /// Each ticket with an unsatisfied blocker: the distinct ids it names
    /// that are not a `Complete` or `Superseded` ticket.
    unsatisfied: BTreeMap<String, u32>,
    /// MNG-4's ready set, by its own rule — `Ready`, and every entry of
    /// `blocked_by` a ticket that is done with — per shortcode.
    members: BTreeMap<String, BTreeSet<u32>>,
    statuses: BTreeMap<(String, &'static str), u64>,
    ready: BTreeMap<String, u64>,
}

fn scan(rows: &[Row]) -> Scan {
    let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
    let resolved = |b: &str| by_id.get(b).is_some_and(|bt| done(bt.status));
    let mut s = Scan {
        unsatisfied: BTreeMap::new(),
        members: BTreeMap::new(),
        statuses: BTreeMap::new(),
        ready: BTreeMap::new(),
    };
    for t in rows {
        let named: BTreeSet<&str> = t.blocked_by.iter().map(String::as_str).collect();
        let open = named.iter().filter(|b| !resolved(b)).count() as u32;
        if open > 0 {
            s.unsatisfied.insert(t.id.clone(), open);
        }
        let member = t.status == Status::Ready && t.blocked_by.iter().all(|b| resolved(b));
        assert_eq!(member, t.status == Status::Ready && open == 0, "the count decides what the rule decides: {}", t.id);
        if member {
            s.members.entry(t.epic.clone()).or_default().insert(t.seq);
            *s.ready.entry(t.epic.clone()).or_default() += 1;
        }
        *s.statuses.entry((t.epic.clone(), t.status.as_str())).or_default() += 1;
    }
    s
}

/// What the cache keeps, in the scan's shape.
fn kept(store: &Store) -> Scan {
    let cache = store.tickets.read().unwrap();
    let (unsatisfied, statuses, ready) = cache.ready.stored();
    Scan {
        unsatisfied,
        members: cache.ready.members().clone(),
        statuses,
        ready,
    }
}

/// What the file holds, read on a read-only connection of its own, in one
/// read transaction: its rows, where its derived state stands, and the
/// kept state as stored — the counts from their column, the ready set from
/// its index, the two tables.
fn filed(path: &Path) -> (Vec<Row>, State, Scan) {
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let tx = conn.transaction().unwrap();
    let mut rows: Vec<Row> = db::load_rows(&tx).unwrap().into_values().collect();
    rows.sort_by_key(OrderKey::of);
    let form = stored::read(&tx).unwrap();
    let members = stored::read_ready(&tx).unwrap();
    let state = stored::state(&tx).unwrap();
    (
        rows,
        state,
        Scan {
            unsatisfied: form.unsatisfied,
            members,
            statuses: form.statuses,
            ready: form.ready,
        },
    )
}

/// Acceptance 1: the kept counts, the ready set and the status counts are
/// a scan's — in the cache, and in the file, whose rows are the cache's.
#[track_caller]
pub(super) fn assert_kept_is_a_scan(store: &Store, path: Option<&Path>, after: &str) {
    let rows = rows(store);
    let want = scan(&rows);
    assert_eq!(kept(store), want, "the cache, after {after}");
    if let Some(path) = path {
        let (stored_rows, state, stored) = filed(path);
        assert_eq!(state, State::Current, "after {after}");
        // WAVE-6: placements are the cache's alone — held to the rule instead.
        let unplaced: Vec<Row> = rows.iter().map(|t| Row { placement: None, ..t.clone() }).collect();
        assert_eq!(stored_rows, unplaced, "the file's rows are the cache's, after {after}");
        super::tests_placements::assert_kept_placements_are_the_rule(store, after);
        assert_eq!(stored, want, "the file, after {after}");
    }
}

/// What a page is, `(rows, next, remaining)`.
type Answer = (Vec<String>, Option<String>, u64);

fn answer(page: Page) -> Answer {
    (page.tickets.into_iter().map(|t| t.id).collect(), page.next, page.remaining)
}

/// What is paged: the ready set, or `list` with a status.
#[derive(Debug, Clone, Copy)]
enum Asked {
    Ready,
    Status(Status),
}

/// The page the code before SCALE-32 gave, run here as it ran there: the
/// span cut as `page` and `ready_page` cut it, then every ticket in it
/// tested — for the ready set, each of its blockers looked up — and
/// everything past the page tested and counted (`Filter::By`, the walk
/// `find` still runs).
fn before(store: &Store, asked: Asked, epic: Option<&str>, after: Option<&str>, limit: Option<u32>) -> Result<Answer, String> {
    let epic = epic.map(str::to_ascii_uppercase);
    let after = after.map(str::to_ascii_uppercase);
    let cache = store.tickets.read().unwrap();
    let span = Span::of(&cache, after.as_deref(), epic.as_deref()).map_err(|e| e.to_string())?;
    let page = match asked {
        Asked::Ready => {
            let keep = |t: &Row| {
                t.status == Status::Ready
                    && t.blocked_by
                        .iter()
                        .all(|b| cache.get(b).is_some_and(|bt| satisfied(bt.status)))
            };
            Store::walk_cache(&cache, span, limit, Filter::By(&keep), Rest::Count)
        }
        Asked::Status(s) => {
            let keep = |t: &Row| t.status == s;
            Store::walk_cache(&cache, span, limit, Filter::By(&keep), Rest::Count)
        }
    };
    Ok(answer(page))
}

/// The store's order as a plain list, read once per state of the store —
/// the rule's whole input: each row, where each id sits, and whether MNG-4's
/// rule, applied as it reads, keeps it.
struct Plain {
    rows: Vec<Row>,
    at: HashMap<String, usize>,
    ready: Vec<bool>,
}

impl Plain {
    fn of(store: &Store) -> Self {
        let rows = rows(store);
        let at: HashMap<String, usize> = rows.iter().enumerate().map(|(i, t)| (t.id.clone(), i)).collect();
        let ready = rows
            .iter()
            .map(|t| t.status == Status::Ready && t.blocked_by.iter().all(|b| at.get(b).is_some_and(|i| done(rows[*i].status))))
            .collect();
        Self { rows, at, ready }
    }

    /// MNG-9's page by MNG-4's rule: the tickets under the epic, past the
    /// cursor, that the rule keeps — the first `limit` the page, the rest
    /// counted.
    fn page(&self, asked: Asked, epic: Option<&str>, after: Option<&str>, limit: Option<u32>) -> Result<Answer, String> {
        let epic = epic.map(str::to_ascii_uppercase);
        let after = after.map(str::to_ascii_uppercase);
        let cursor = match &after {
            None => None,
            Some(id) => match self.at.get(id) {
                Some(i) => Some(OrderKey::of(&self.rows[*i])),
                None => return Err(format!("after: no ticket {id}")),
            },
        };
        let matching: Vec<&Row> = self
            .rows
            .iter()
            .zip(&self.ready)
            .filter(|(t, _)| epic.as_deref().is_none_or(|e| t.epic == e))
            .filter(|(t, _)| cursor.as_ref().is_none_or(|c| OrderKey::of(t) > *c))
            .filter(|(t, ready)| match asked {
                Asked::Ready => **ready,
                Asked::Status(s) => t.status == s,
            })
            .map(|(t, _)| t)
            .collect();
        let take = limit.map_or(matching.len(), |n| (n as usize).min(matching.len()));
        let page: Vec<String> = matching[..take].iter().map(|t| t.id.clone()).collect();
        let remaining = (matching.len() - take) as u64;
        let next = if remaining == 0 { None } else { page.last().cloned().or(after) };
        Ok((page, next, remaining))
    }
}

/// What the store answers now.
fn answered(store: &Store, asked: Asked, epic: Option<&str>, after: Option<&str>, limit: Option<u32>) -> Result<Answer, String> {
    let page = match asked {
        Asked::Ready => store.ready_page(epic, after, limit),
        Asked::Status(status) => store.page(PageQuery {
            epic,
            status: Some(status),
            after,
            limit,
            ..PageQuery::default()
        }),
    };
    page.map(answer).map_err(|e| e.to_string())
}

/// Acceptance 2, for one page: the store's answer is the code before's, and
/// both are the rule's.
#[track_caller]
fn assert_page(store: &Store, plain: &Plain, asked: Asked, epic: Option<&str>, after: Option<&str>, limit: Option<u32>, what: &str) {
    let now = answered(store, asked, epic, after, limit);
    assert_eq!(now, before(store, asked, epic, after, limit), "{what}: {asked:?} epic {epic:?} after {after:?} limit {limit:?}");
    assert_eq!(now, plain.page(asked, epic, after, limit), "{what}: {asked:?} epic {epic:?} after {after:?} limit {limit:?}");
}

/// A sample of the epics worth filtering on (none, some held, a lowercase
/// spelling, one that holds nothing) and of the cursors worth trying (none,
/// some shortcodes' first and last tickets, some at random, one in
/// lowercase, one that names no ticket) — a different sample each time.
fn epics_and_cursors(plain: &Plain, rng: &mut Rng) -> (Vec<Option<String>>, Vec<Option<String>>) {
    let mut by_epic: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for t in &plain.rows {
        by_epic.entry(t.epic.as_str()).or_default().push(t.id.as_str());
    }
    let held: Vec<&str> = by_epic.keys().copied().collect();
    let mut epics: Vec<Option<String>> = vec![None, Some("NOTHING".into())];
    for i in 0..3.min(held.len()) {
        let e = held[rng.below(held.len())];
        epics.push(Some(if i == 2 { e.to_lowercase() } else { e.to_string() }));
    }
    let mut cursors: Vec<Option<String>> = vec![None, Some("NO-SUCH-1".into())];
    for _ in 0..8.min(held.len()) {
        let run = &by_epic[held[rng.below(held.len())]];
        cursors.push(Some(run[0].to_string()));
        cursors.push(Some(run[run.len() - 1].to_string()));
    }
    for _ in 0..6 {
        if !plain.rows.is_empty() {
            let id = &plain.rows[rng.below(plain.rows.len())].id;
            cursors.push(Some(if rng.below(4) == 0 { id.to_lowercase() } else { id.clone() }));
        }
    }
    (epics, cursors)
}

/// Acceptance 2 for a store as it stands: for each epic × cursor, the ready
/// set at every limit and each status at one (a different one each time);
/// then whole walks, page by page.
#[track_caller]
fn assert_every_page_is_before(store: &Store, rng: &mut Rng, what: &str) {
    let plain = Plain::of(store);
    let (epics, cursors) = epics_and_cursors(&plain, rng);
    let limits = [None, Some(0), Some(1), Some(3), Some(200)];
    for epic in &epics {
        for after in &cursors {
            for limit in limits {
                assert_page(store, &plain, Asked::Ready, epic.as_deref(), after.as_deref(), limit, what);
            }
            for status in Status::ALL {
                let limit = limits[rng.below(limits.len())];
                assert_page(store, &plain, Asked::Status(status), epic.as_deref(), after.as_deref(), limit, what);
            }
        }
    }
    for a in [Asked::Ready, Asked::Status(Status::Ready), Asked::Status(Status::Complete)] {
        for epic in [None, epics.get(2).cloned().flatten()] {
            let limit = 1 + rng.below(4) as u32;
            let mut after: Option<String> = None;
            let mut seen = 0;
            loop {
                assert_page(store, &plain, a, epic.as_deref(), after.as_deref(), Some(limit), what);
                let Ok((page, next, _)) = answered(store, a, epic.as_deref(), after.as_deref(), Some(limit)) else {
                    break;
                };
                seen += page.len();
                match next {
                    Some(next) => after = Some(next),
                    None => break,
                }
            }
            let (all, _, _) = plain.page(a, epic.as_deref(), None, None).unwrap();
            // A walk ends early only at an id no cursor can name again
            // (lowercase: only a hand edit writes one — SCALE-19, "found").
            if all.iter().all(|id| *id == id.to_ascii_uppercase()) {
                assert_eq!(seen, all.len(), "{what}: a walk of {a:?} in {epic:?} visits every member once");
            }
        }
    }
}

/// How many writes of each kind changed something, for the walk's own check
/// that it exercised the ones that move membership.
#[derive(Debug, Default)]
pub(super) struct Tally {
    pub(super) created: usize,
    with_blockers: usize,
    forward: usize,
    resolved: usize,
    pub(super) crossings: usize,
    fanned: usize,
    joined: usize,
    left: usize,
    pub(super) moves: usize,
    others: usize,
    /// SDLC-11: docs born, and creates tried (what picks a doc).
    docs: usize,
    attempts: usize,
    /// SDLC-13: writes made (what picks a `set_kind`), kinds changed, and of
    /// those, the ones that re-rooted a shortcode.
    writes: usize,
    pub(super) kinds: usize,
    pub(super) reroots: usize,
    /// FILTER-21: whether some docs are filed as menger's own — a walk
    /// that holds filters or cells to a scan asks for them; the others walk
    /// exactly as they did. And how many were born.
    pub(super) file_menger_docs: bool,
    pub(super) mengers: usize,
    /// MENGER-12: whether the walk also asks and labels — bodies that open,
    /// re-declare and drop inputs, answers, clears, closes and reopens, and
    /// labels added and removed; opt-in, as the board notes are, so the
    /// other walks walk as they did. And how many of each moved something.
    pub(super) ask_and_label: bool,
    pub(super) asked: usize,
    pub(super) labelled: usize,
}

impl Tally {
    /// FILTER-21: a walk that files board notes among its docs.
    pub(super) fn filing_menger_docs() -> Self {
        Self { file_menger_docs: true, ..Self::default() }
    }

    /// MENGER-12: a walk that files board notes, asks and labels.
    pub(super) fn asking_and_labelling() -> Self {
        Self { file_menger_docs: true, ask_and_label: true, ..Self::default() }
    }
}

/// MENGER-12: one write that moves `asking` or a label — on draws of its own
/// (`own`), so the walk's draws stay the ones it had. A body that declares a
/// text input and a decision, or one of them, or none; an answer, a clear, a
/// close or a reopen of one of a ticket's inputs (a refusal — a closed or
/// removed input — is a write too); a label added or removed.
pub(super) async fn random_ask_or_label(store: &Store, own: &mut Rng, all: &[Row], tally: &mut Tally) -> String {
    let id = all[own.below(all.len())].id.clone();
    let text = "<m-input id=\"q\" kind=\"text\"><m-question>Anything else?</m-question></m-input>\n";
    let decision = "<m-decision data-field=\"decision\" id=\"d\">\n\n<m-question data-field=\"question\">Which?</m-question>\n\n\
                    <m-option data-field=\"option\" name=\"A\" value=\"a\"></m-option>\n\
                    <m-option data-field=\"option\" name=\"B\" value=\"b\"></m-option>\n\n</m-decision>\n";
    let held: Vec<String> = store.inputs(Some(&id), None, None).unwrap().into_iter().map(|i| i.id).collect();
    match own.below(10) {
        0..=2 => {
            let body = match own.below(4) {
                0 => format!("asks\n\n{text}\n{decision}"),
                1 => format!("asks\n\n{text}"),
                2 => format!("decides\n\n{decision}"),
                _ => "asks nothing".to_string(),
            };
            store.amend(&id, None, Some(body)).await.unwrap();
            tally.asked += 1;
            format!("amend {id}'s inputs")
        }
        3..=6 if !held.is_empty() => {
            let input = held[own.below(held.len())].clone();
            let done = match own.below(4) {
                0 => {
                    let value = match input.as_str() {
                        "q" => InputValue::Text { text: "yes".into() },
                        _ => InputValue::Choice { option: "a".into() },
                    };
                    store.answer(&id, &input, value, None, crate::types::AnswerSource::Api).await
                }
                1 => store.clear_answer(&id, &input, crate::types::AnswerSource::Api).await,
                2 => store.close_input(&id, &input).await,
                _ => store.reopen_input(&id, &input).await,
            };
            tally.asked += usize::from(matches!(done, Ok(InputWrite::Changed(_))));
            format!("an input write to {id}'s {input}")
        }
        _ => {
            let names = ["needs-you", "p1", "later"];
            let (add, remove): (Vec<String>, Vec<String>) = match own.below(3) {
                0 => (vec![names[own.below(3)].into()], vec![]),
                1 => (vec![], vec![names[own.below(3)].into()]),
                _ => (vec![names[own.below(3)].into(), names[own.below(3)].to_ascii_uppercase()], vec![names[own.below(3)].into()]),
            };
            match store.label(&id, add.clone(), remove.clone()).await.unwrap() {
                Change::LabelsChanged { added, removed, .. } if !(added.is_empty() && removed.is_empty()) => tally.labelled += 1,
                _ => {}
            }
            format!("label {id} +{add:?} -{remove:?}")
        }
    }
}

/// One random write against the store as it stands, weighted to what moves
/// the ready set.
pub(super) async fn random_write(store: &Store, rng: &mut Rng, epics: &[&str], tally: &mut Tally) -> String {
    // Ids the API can name (a lowercase one is folded by `amend` and its kin).
    let all: Vec<Row> = rows(store).into_iter().filter(|t| t.id == t.id.to_ascii_uppercase()).collect();
    let extents: BTreeMap<String, u32> = store.extents().into_iter().collect();
    let pick = |rng: &mut Rng| all[rng.below(all.len())].id.clone();
    let members_before = store.tickets.read().unwrap().ready.size();
    // SDLC-13: every seventh write a kind changed — every pair of kinds, with
    // and without a status, legal or refused, held to the rules
    // (`tests_kinds`). By a count, and on draws of its own: the walk's draws
    // stay the ones it had.
    tally.writes += 1;
    if !all.is_empty() && tally.writes % 7 == 0 {
        let mut own = Rng(rng.0 ^ (tally.writes as u64).wrapping_mul(0x5D1C_0013_0000_0001));
        let (what, did) = super::tests_kinds::random_set_kind(store, &mut |n| own.below(n)).await;
        match did {
            super::tests_kinds::KindWrite::Changed { reroot } => {
                tally.kinds += 1;
                tally.reroots += usize::from(reroot);
            }
            _ => tally.others += 1,
        }
        let members_after = store.tickets.read().unwrap().ready.size();
        tally.joined += usize::from(members_after > members_before);
        tally.left += usize::from(members_after < members_before);
        return what;
    }
    // MENGER-12: every fifth write, when the walk asks for them, one that
    // asks or labels — by the count, on draws of its own.
    if tally.ask_and_label && !all.is_empty() && tally.writes % 5 == 3 {
        let mut own = Rng(rng.0 ^ (tally.writes as u64).wrapping_mul(0x4E12_0012_0000_0001));
        return random_ask_or_label(store, &mut own, &all, tally).await;
    }
    let what = match if all.is_empty() { 0 } else { rng.below(20) } {
        0..=6 => {
            let epic = epics[rng.below(epics.len())];
            let exists = extents.contains_key(epic);
            let kind = if exists { Kind::Build } else { Kind::Scope };
            // SDLC-11: every eighth create a doc — never ready, never blocked,
            // never a blocker: what names one, or would be named before it,
            // is refused. By a count, not a draw: the walk's draws stay the
            // ones it had.
            tally.attempts += 1;
            let kind = if tally.attempts % 8 == 3 { Kind::Doc } else { kind };
            let mine: Vec<&Row> = all.iter().filter(|t| t.epic == epic).collect();
            let parent = match rng.below(4) {
                0 if !mine.is_empty() => Some(mine[rng.below(mine.len())].id.clone()),
                1 if !all.is_empty() => Some(pick(rng)),
                _ => None,
            };
            // Blockers: tickets that exist, ids not born yet (the next few of
            // some shortcode — born later in the walk, or not), one that never
            // will be, and one named twice.
            let mut blocked_by: Vec<String> = Vec::new();
            for _ in 0..rng.below(4) {
                blocked_by.push(match rng.below(10) {
                    0..=5 if !all.is_empty() => pick(rng),
                    0..=7 => {
                        let e = epics[rng.below(epics.len())];
                        format!("{e}-{}", extents.get(e).copied().unwrap_or(0) + 1 + rng.below(3) as u32)
                    }
                    8 => "NEVER-0".to_string(),
                    _ => blocked_by.last().cloned().unwrap_or_else(|| "NEVER-0".into()),
                });
            }
            if kind == Kind::Doc && !tally.attempts.is_multiple_of(3) {
                blocked_by.clear();
            }
            // FILTER-21: when the walk asks, every other doc one of menger's
            // own — a path under `_MENGER/`, filed under the shortcode's
            // root (refused where it has none). By the count again: the
            // draws stay the walk's.
            let menger = tally.file_menger_docs && kind == Kind::Doc && tally.attempts % 16 == 11;
            let path = menger.then(|| format!("_MENGER/note-{}", tally.attempts));
            let parent = if path.is_some() { None } else { parent };
            if path.is_some() {
                blocked_by.clear();
            }
            let next = format!("{}-{}", epic.to_ascii_uppercase(), extents.get(epic).copied().unwrap_or(0) + 1);
            let resolves = !store.tickets.read().unwrap().ids.waiting_on(&next).is_empty();
            let forward = blocked_by.iter().any(|b| !all.iter().any(|t| &t.id == b));
            let what = format!("create {epic} under {parent:?} blocked by {blocked_by:?} at {path:?}");
            // A refusal (a parent outside the shortcode) is a write too.
            let new = NewTicket { path: path.clone(), ..new_ticket(epic, kind, parent, blocked_by.clone()) };
            if store.create(new).await.is_ok() {
                tally.created += 1;
                tally.mengers += usize::from(path.is_some());
                tally.docs += usize::from(kind == Kind::Doc);
                tally.with_blockers += usize::from(!blocked_by.is_empty());
                tally.forward += usize::from(forward);
                tally.resolved += usize::from(resolves);
            }
            what
        }
        7..=13 => {
            // Half the time a ticket that blocks something, so crossings fan out.
            let blockers: Vec<&Row> = all.iter().filter(|t| !t.unlocks.is_empty()).collect();
            let id = if !blockers.is_empty() && rng.below(2) == 0 {
                blockers[rng.below(blockers.len())].id.clone()
            } else {
                pick(rng)
            };
            let status = match rng.below(10) {
                0..=2 => Status::Complete,
                3 => Status::Superseded,
                4..=6 => Status::Ready,
                _ => Status::ALL[rng.below(Status::ALL.len())],
            };
            let row = store.row(&id).unwrap().0;
            match store.set_status(&id, status).await {
                Ok(_) if done(row.status) != done(status) => {
                    tally.crossings += 1;
                    tally.fanned += usize::from(!row.unlocks.is_empty());
                }
                Ok(_) => {}
                // SDLC-11: a doc's status never moves; Doc is a doc's alone.
                Err(StoreError::DocHasNoLifecycle { .. }) if row.kind == Kind::Doc => tally.others += 1,
                Err(StoreError::DocStatusOnWork { .. }) if status == Status::Doc => tally.others += 1,
                Err(e) => panic!("set_status {id} {status}: {e}"),
            }
            format!("set_status {id} {} -> {status}", row.status)
        }
        14..=16 => {
            // Inside the child's shortcode, mostly; anywhere (refused, but a
            // root's) otherwise.
            let child = pick(rng);
            let epic = store.row(&child).unwrap().0.epic;
            let mine: Vec<&Row> = all.iter().filter(|t| t.epic == epic).collect();
            let parent = if rng.below(3) > 0 { mine[rng.below(mine.len())].id.clone() } else { pick(rng) };
            let was = store.row(&child).unwrap().0.parent;
            match store.set_parent(&child, &parent).await {
                Ok(_) if was.as_deref() != Some(parent.as_str()) => tally.moves += 1,
                Ok(_) | Err(_) => {}
            }
            format!("set_parent {child} under {parent}")
        }
        17 => {
            let id = pick(rng);
            store.amend(&id, Some(format!("amended {}", rng.below(99))), None).await.unwrap();
            tally.others += 1;
            format!("amend {id}")
        }
        18 => {
            let id = pick(rng);
            store.link_commits(&id, vec![format!("{:07x}", 0xabc0000 + rng.below(4))]).await.unwrap();
            tally.others += 1;
            format!("link_commits {id}")
        }
        _ => {
            let id = pick(rng);
            store.unlink_commits(&id, vec![format!("{:07x}", 0xabc0000 + rng.below(4))]).await.unwrap();
            tally.others += 1;
            format!("unlink_commits {id}")
        }
    };
    let members_after = store.tickets.read().unwrap().ready.size();
    tally.joined += usize::from(members_after > members_before);
    tally.left += usize::from(members_after < members_before);
    what
}

/// `writes` random writes, the kept state held to a scan — in the cache
/// and in the file — after each, and every page to the code before every
/// `pages` writes; then a reopen, which reads what was kept.
async fn walk(store: Store, path: &Path, seed: u64, writes: usize, pages: usize, epics: &[&str]) -> Store {
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    for step in 0..writes {
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        assert_kept_is_a_scan(&store, Some(path), &format!("step {step}: {what}"));
        if step % pages == pages - 1 {
            assert_every_page_is_before(&store, &mut rng, &format!("step {step}"));
        }
    }
    assert!(
        tally.created > writes / 5
            && tally.with_blockers > writes / 20
            && tally.forward > 0
            && tally.resolved > 0
            && tally.crossings > writes / 20
            && tally.fanned > 0
            && tally.joined > 0
            && tally.left > 0
            && tally.moves > 0
            && tally.others > 0
            && tally.docs > 0
            && tally.kinds > writes / 50,
        "every kind of write, and enough of those that move the ready set: {tally:?}"
    );
    let before = kept(&store);
    drop(store);
    let store = Store::open(path).unwrap();
    assert_eq!(kept(&store), before, "a reopen reads what was kept");
    assert_kept_is_a_scan(&store, Some(path), "the reopen");
    assert_every_page_is_before(&store, &mut rng, "the reopen");
    store
}

/// Acceptance 1 and 2, on a store built by `create` — and by every other
/// write — from nothing.
#[tokio::test]
async fn the_kept_state_is_a_scan_and_every_page_is_the_old_one_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    assert_kept_is_a_scan(&store, Some(&path), "a fresh open");
    walk(store, &path, 0x5CA1_E032, 450, 50, &["AA", "MAP", "SUB", "ZED"]).await;
}

/// Acceptance 1 and 2, on a seeded store, and through a seed into a store
/// already walked: a seed counts it all once, and stores it whole.
#[tokio::test]
async fn the_kept_state_is_a_scan_and_every_page_is_the_old_one_on_a_seeded_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seeded.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 1_200, seed: 32, into_existing: false }).unwrap();
    assert_kept_is_a_scan(&store, Some(&path), "a seed");
    let mut rng = Rng(0x5EED);
    assert_every_page_is_before(&store, &mut rng, "a seed");
    let epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).take(4).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    let store = walk(store, &path, 0x5EED_0032, 250, 125, &epics).await;
    store.seed_synthetic(SeedSpec { count: 600, seed: 5, into_existing: true }).unwrap();
    assert_kept_is_a_scan(&store, Some(&path), "a seed into the walked store");
    assert_every_page_is_before(&store, &mut rng, "a seed into the walked store");
    walk(store, &path, 0x5EED_0033, 120, 60, &epics).await;
}

/// Acceptance 1 and 2, on every fixture a binary from before SCALE-28 wrote
/// (each meeting the migrations of its age first): the first open counts it
/// all, and every write after keeps it so.
#[tokio::test]
async fn the_kept_state_is_a_scan_and_every_page_is_the_old_one_on_each_fixture() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        let store = Store::open(&path).unwrap();
        assert_kept_is_a_scan(&store, Some(&path), &format!("the first open of {name}"));
        let mut rng = Rng(i as u64);
        assert_every_page_is_before(&store, &mut rng, name);
        let mut epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        walk(store, &path, 0xF132 + i as u64, 150, 50, &epics).await;
    }
}

/// Acceptance 1 and 2 on what only a hand edit makes — parent cycles and a
/// parent that names no ticket (MNG-17), holes, a sequence 0 and a lowercase
/// shortcode (SCALE-19) — with blockers and statuses of every kind spread
/// over them.
#[tokio::test]
async fn the_kept_state_is_a_scan_and_every_page_is_the_old_one_on_hand_edited_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    tests::write_db(&path, &tests::legacy());
    {
        // Blockers over the legacy shapes, mirrored as the store keeps them.
        let conn = Connection::open(&path).unwrap();
        let ids: Vec<String> = conn
            .prepare("SELECT id FROM tickets ORDER BY epic, seq")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut rng = Rng(0x1E6A);
        for (i, id) in ids.iter().enumerate() {
            let status = Status::ALL[rng.below(Status::ALL.len())];
            let blocked_by: Vec<String> = (0..rng.below(3)).filter(|_| i > 0).map(|_| ids[rng.below(i)].clone()).collect();
            conn.execute(
                "UPDATE tickets SET status = ?2, blocked_by = ?3 WHERE id = ?1",
                rusqlite::params![id, status.as_str(), serde_json::to_string(&blocked_by).unwrap()],
            )
            .unwrap();
            for b in blocked_by.iter().collect::<BTreeSet<_>>() {
                conn.execute(
                    "UPDATE tickets SET unlocks = json_insert(unlocks, '$[#]', ?2) WHERE id = ?1",
                    rusqlite::params![b, id],
                )
                .unwrap();
            }
        }
    }
    let store = Store::open(&path).unwrap();
    assert_kept_is_a_scan(&store, Some(&path), "the first open");
    let store = walk(
        store,
        &path,
        0x4A4D_0032,
        250,
        50,
        &["AAA", "AGS", "CYC", "DEMO", "NOSC", "PLEX", "TRAN", "XPC", "NEW"],
    )
    .await;
    drop(store);

    let path = dir.path().join("holes.db");
    tests_paging::write_runs(&path, &[("HOLE", 1), ("HOLE", 2), ("HOLE", 5), ("ZERO", 0), ("ZERO", 1), ("low", 1), ("low", 2)]);
    Connection::open(&path)
        .unwrap()
        .execute_batch("UPDATE tickets SET status = 'Ready'; UPDATE tickets SET blocked_by = '[\"HOLE-3\"]' WHERE id = 'low-2';")
        .unwrap();
    let store = Store::open(&path).unwrap();
    assert_kept_is_a_scan(&store, Some(&path), "a file with holes");
    let mut rng = Rng(7);
    assert_every_page_is_before(&store, &mut rng, "holes");
    walk(store, &path, 0x401E_0032, 120, 40, &["HOLE", "ZERO", "NEW"]).await;
}

/// The three writes that move membership, one at a time, with the numbers
/// the ticket names: a status change of the ticket itself; a status change
/// of a blocker across the satisfied line (and not across it: `Complete` to
/// `Superseded` moves no count); a birth with blockers; and a birth that
/// resolves forward references — whose referrers' counts do not move, the
/// newborn being `Pending`, until it is done.
#[tokio::test]
async fn membership_moves_on_exactly_the_three_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("three.db");
    let store = Store::open(&path).unwrap();
    let ids = |store: &Store| -> Vec<String> { store.ready_page(None, None, None).unwrap().tickets.into_iter().map(|t| t.id).collect() };
    let count = |store: &Store, id: &str| store.tickets.read().unwrap().ready.counts().get(id).copied().unwrap_or(0);
    store.create(new_ticket("TH", Kind::Scope, None, vec![])).await.unwrap();
    // TH-2 names TH-5 (not born) twice, and TH-1.
    let t2 = store.create(new_ticket("TH", Kind::Build, None, vec!["TH-5".into(), "TH-1".into(), "TH-5".into()])).await.unwrap();
    assert_eq!(count(&store, &t2.id), 2, "two distinct blockers, neither done; the unborn one counts once");
    store.create(new_ticket("TH", Kind::Build, None, vec!["TH-1".into()])).await.unwrap();
    store.create(new_ticket("TH", Kind::Build, None, vec![])).await.unwrap();
    assert_kept_is_a_scan(&store, Some(&path), "the setup");

    // The ticket itself.
    store.set_status("TH-4", Status::Ready).await.unwrap();
    assert_eq!(ids(&store), ["TH-4"]);
    store.set_status("TH-3", Status::Ready).await.unwrap();
    store.set_status("TH-2", Status::Ready).await.unwrap();
    assert_eq!(ids(&store), ["TH-4"], "TH-3 and TH-2 wait on TH-1");

    // A blocker across the line: TH-1 unlocks TH-2 and TH-3.
    let fanned = |store: &Store, id: &str, status: Status| {
        let cache = store.tickets.read().unwrap();
        let mut row = cache.get(id).unwrap().clone();
        let previous = row.status;
        row.status = status;
        cache.ready.status_changed(cache.rows().with(&row), &row, previous).counts_moved()
    };
    assert_eq!(fanned(&store, "TH-1", Status::Complete), 2, "a crossing moves each ticket that names it");
    store.set_status("TH-1", Status::Complete).await.unwrap();
    assert_eq!(ids(&store), ["TH-3", "TH-4"], "TH-2 still waits on TH-5, which is not born");
    assert_eq!(count(&store, "TH-2"), 1);
    assert_eq!(fanned(&store, "TH-1", Status::Superseded), 0, "Complete to Superseded is not a crossing");
    store.set_status("TH-1", Status::Superseded).await.unwrap();
    assert_eq!(ids(&store), ["TH-3", "TH-4"]);
    assert_kept_is_a_scan(&store, Some(&path), "the crossings");

    // A birth that resolves a forward reference: TH-5 is born `Pending`, so
    // TH-2's count does not move — and the scan agrees.
    let t5 = store.create(new_ticket("TH", Kind::Build, None, vec!["TH-4".into()])).await.unwrap();
    assert_eq!(t5.unlocks, ["TH-2"]);
    assert_eq!(count(&store, "TH-2"), 1, "the newborn is not done: TH-2 waits on it still");
    assert_eq!(count(&store, "TH-5"), 1, "TH-4 is Ready, not done");
    assert_kept_is_a_scan(&store, Some(&path), "a birth that resolves a forward reference");
    store.set_status("TH-5", Status::Complete).await.unwrap();
    assert_eq!(ids(&store), ["TH-2", "TH-3", "TH-4"], "now it is");
    assert_eq!(count(&store, "TH-2"), 0);

    // And back across the line.
    store.set_status("TH-1", Status::Ready).await.unwrap();
    assert_eq!(ids(&store), ["TH-1", "TH-4"]);
    assert_eq!((count(&store, "TH-2"), count(&store, "TH-3")), (1, 1));
    assert_kept_is_a_scan(&store, Some(&path), "back across the line");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(ids(&store), ["TH-1", "TH-4"], "a reopen reads it");
}

/// The fan-out: a ticket that blocks k others moves k counts in one write,
/// and they all enter the ready set together — then leave it together.
/// The one transaction carries all of them: the file agrees after each.
#[tokio::test]
async fn completing_a_ticket_that_blocks_many_moves_each_of_them_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fan.db");
    let store = Store::open(&path).unwrap();
    store.create(new_ticket("FAN", Kind::Scope, None, vec![])).await.unwrap();
    let hub = store.create(new_ticket("FAN", Kind::Build, None, vec![])).await.unwrap().id;
    let other = store.create(new_ticket("FAN", Kind::Build, None, vec![])).await.unwrap().id;
    let k: usize = 120;
    for i in 0..k {
        // A third also wait on another ticket; every one is Ready.
        let blocked_by = if i % 3 == 0 { vec![hub.clone(), other.clone()] } else { vec![hub.clone()] };
        let id = store.create(new_ticket("FAN", Kind::Build, None, blocked_by)).await.unwrap().id;
        store.set_status(&id, Status::Ready).await.unwrap();
    }
    let ready = |store: &Store| store.ready_page(Some("fan"), None, Some(0)).unwrap().remaining;
    assert_eq!(ready(&store), 0);
    store.set_status(&hub, Status::Complete).await.unwrap();
    assert_eq!(ready(&store), (k - k.div_ceil(3)) as u64, "every one blocked by nothing else joins");
    assert_kept_is_a_scan(&store, Some(&path), "the fan-out");
    store.set_status(&other, Status::Superseded).await.unwrap();
    assert_eq!(ready(&store), k as u64);
    store.set_status(&hub, Status::Coding).await.unwrap();
    assert_eq!(ready(&store), 0, "and back: every one leaves");
    assert_kept_is_a_scan(&store, Some(&path), "the fan-out, back");
}

/// SDLC-15: `Canceled` is done with, as `Superseded` is — a ticket whose
/// one blocker is canceled is ready, its placement is `done`, its category
/// `dropped`, a roll-up's `done` does not count it — and it is kept as the
/// text it is written in: a reopen reads it back.
#[tokio::test]
async fn a_canceled_blocker_is_done_with_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("canceled.db");
    let store = Store::open(&path).unwrap();
    store.create(new_ticket("CAN", Kind::Scope, None, vec![])).await.unwrap();
    let dropped = store.create(new_ticket("CAN", Kind::Build, None, vec![])).await.unwrap().id;
    let waiting = store.create(new_ticket("CAN", Kind::Build, None, vec![dropped.clone()])).await.unwrap().id;
    store.set_status(&waiting, Status::Ready).await.unwrap();
    let ready = |store: &Store| store.ready_page(Some("can"), None, Some(0)).unwrap().remaining;
    assert_eq!(ready(&store), 0);
    store.set_status(&dropped, Status::Canceled).await.unwrap();
    assert_eq!(ready(&store), 1, "a canceled blocker is satisfied");
    assert_eq!(crate::filter::category_of(Status::Canceled), crate::filter::Category::Dropped);
    assert_kept_is_a_scan(&store, Some(&path), "canceled");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(&dropped).await.unwrap().status, Status::Canceled, "read back as written");
    assert_eq!(ready(&store), 1);
    assert_kept_is_a_scan(&store, Some(&path), "canceled, reopened");
}

/// What the fan-out relies on: a blocker's `unlocks` names each ticket that
/// names it — the mirror `create` and the seed keep. A file edited by hand
/// can break it. An entry named twice, one that names no ticket and one for
/// a ticket that does not name the blocker are passed over; a ticket the
/// mirror leaves out is not found, so its count is not moved — the one
/// place the kept state can part from the scan, and `--verify-derived`
/// names it.
#[tokio::test]
async fn a_hand_edit_that_breaks_the_unlocks_mirror_is_named_by_verify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mirror.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(db::SCHEMA).unwrap();
    for (id, seq, blocked_by, unlocks) in [
        ("MI-1", 1, "[]", r#"["MI-3","MI-3","GHOST-9","MI-4"]"#), // leaves MI-2 out
        ("MI-2", 2, r#"["MI-1"]"#, "[]"),
        ("MI-3", 3, r#"["MI-1"]"#, "[]"),
        ("MI-4", 4, "[]", "[]"),
    ] {
        conn.execute(
            "INSERT INTO tickets (id, epic, seq, title, status, kind, blocked_by, unlocks, body, created_unix, updated_unix)
                 VALUES (?1, 'MI', ?2, ?1, 'Ready', 'build', ?3, ?4, '', 0, 0)",
            rusqlite::params![id, seq, blocked_by, unlocks],
        )
        .unwrap();
    }
    drop(conn);
    let store = Store::open(&path).unwrap();
    assert_kept_is_a_scan(&store, Some(&path), "the first open, a scan");
    store.set_status("MI-1", Status::Complete).await.unwrap();
    let ids: Vec<String> = store.ready_page(None, None, None).unwrap().tickets.into_iter().map(|t| t.id).collect();
    assert_eq!(ids, ["MI-3", "MI-4"], "MI-3 once; MI-4 does not name MI-1 and is not moved; MI-2 is not found");
    drop(store);
    let Verdict::Differs(found) = Store::verify_derived(&path).unwrap() else {
        panic!("MI-2's count is behind the scan");
    };
    assert_eq!(
        found,
        [
            "unsatisfied blockers of MI-2: stored 1; a scan gives none",
            "ready count MI: stored 2; a scan gives 3",
            // FILTER-4: MI-2's cell follows its kept count — blocked — so
            // the cells part from the scan with it, and are named too.
            "cell_bits MI cell 0 (Ready, build, none, not blocked, no commits) container 0: stored 2 members [3, 4]; a scan gives 3 members [2, 3, 4]",
            "cell_bits MI cell 1 (Ready, build, none, blocked, no commits) container 0: stored 1 members [2]; a scan gives none",
            "cell_counts MI cell 0 (Ready, build, none, not blocked, no commits): stored 2; a scan gives 3",
            "cell_counts MI cell 1 (Ready, build, none, blocked, no commits): stored 1; a scan gives none",
            "ready set MI-2: a scan holds it; the index does not",
        ],
        "{found:#?}"
    );
}

/// Required behavior, "a database written before this ticket": a SCALE-28
/// file — form 1, no `unsatisfied` column, no ready index, no status tables
/// — has them built on its first open, once, and says so; it then verifies;
/// and the next open reads them.
#[tokio::test]
async fn a_file_from_scale_28_gains_the_ready_state_once_on_first_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form1.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 800, seed: 28, into_existing: false }).unwrap();
    let want = kept(&store);
    let pages: Vec<Answer> = [None, Some(3)]
        .into_iter()
        .map(|limit| answer(store.ready_page(None, None, limit).unwrap()))
        .collect();
    drop(store);
    // What SCALE-28 left in a file: its own tables and form, nothing of ours.
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP INDEX tickets_ready; ALTER TABLE tickets DROP COLUMN unsatisfied;
             DROP TABLE status_counts; DROP TABLE ready_counts; UPDATE derived_state SET form = 1;",
        )
        .unwrap();
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::NotStored(why) if why.contains("form 1")));
    let store = Store::open(&path).unwrap();
    assert_eq!(kept(&store), want, "built on first open");
    assert_kept_is_a_scan(&store, Some(&path), "the first open of a form-1 file");
    let again: Vec<Answer> = [None, Some(3)]
        .into_iter()
        .map(|limit| answer(store.ready_page(None, None, limit).unwrap()))
        .collect();
    assert_eq!(again, pages);
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 800, .. }));

    // The next open reads: a number changed by hand between two opens is
    // answered as changed — a rebuild would have answered the scan's.
    let (epic, status, held) = {
        let conn = Connection::open(&path).unwrap();
        let row: (String, String, u64) = conn
            .query_row("SELECT epic, status, held FROM status_counts WHERE status = 'Pending' LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        conn.execute("UPDATE status_counts SET held = held + 7 WHERE epic = ?1 AND status = ?2", rusqlite::params![row.0, row.1]).unwrap();
        row
    };
    let store = Store::open(&path).unwrap();
    let page = store
        .page(PageQuery { epic: Some(&epic), status: Some(status.parse().unwrap()), limit: Some(0), ..PageQuery::default() })
        .unwrap();
    assert_eq!(page.remaining, held + 7, "the status count as stored");
}

/// `--verify-derived` names each new structure where a stored number is
/// changed by hand — a count, a status count, a shortcode's share of the
/// ready set — and a member the index holds that the rule does not.
#[tokio::test]
async fn verify_names_each_altered_number_of_the_ready_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify.db");
    let store = Store::open(&path).unwrap();
    store.create(new_ticket("VR", Kind::Scope, None, vec![])).await.unwrap();
    store.create(new_ticket("VR", Kind::Build, None, vec!["VR-1".into()])).await.unwrap();
    store.create(new_ticket("VR", Kind::Build, None, vec![])).await.unwrap();
    store.set_status("VR-2", Status::Ready).await.unwrap();
    store.set_status("VR-3", Status::Ready).await.unwrap();
    drop(store);
    assert!(matches!(
        Store::verify_derived(&path).unwrap(),
        Verdict::Consistent { tickets: 3, blocked: 1, ready: 1, .. }
    ));
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "UPDATE tickets SET unsatisfied = 0 WHERE id = 'VR-2';
             UPDATE status_counts SET held = 9 WHERE epic = 'VR' AND status = 'Pending';",
        )
        .unwrap();
    let Verdict::Differs(found) = Store::verify_derived(&path).unwrap() else {
        panic!("altered numbers differ from a scan");
    };
    assert_eq!(
        found,
        [
            "unsatisfied blockers of VR-2: none stored; a scan gives 1",
            "status count VR Pending: stored 9; a scan gives 1",
            "ready set VR-2: the index holds it; a scan does not",
        ],
        "{found:#?}"
    );
    Connection::open(&path).unwrap().execute("UPDATE ready_counts SET held = 5", []).unwrap();
    let Verdict::Differs(found) = Store::verify_derived(&path).unwrap() else { unreachable!() };
    assert!(found.contains(&"ready count VR: stored 5; a scan gives 1".to_string()), "{found:#?}");
}

/// The ready set's index is what the file answers a page of `ready_set`
/// from — B4's query, a range of it — and it gives the store's page.
#[tokio::test]
async fn a_page_of_the_ready_set_is_a_range_of_the_files_index() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 2_000, seed: 12, into_existing: false }).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let plan: String = conn
        .query_row(
            "EXPLAIN QUERY PLAN SELECT id FROM tickets WHERE status = 'Ready' AND unsatisfied = 0
                 AND (epic, seq) > (?1, ?2) ORDER BY epic, seq LIMIT 50",
            rusqlite::params!["", -1],
            |r| r.get(3),
        )
        .unwrap();
    assert!(plan.contains("tickets_ready"), "SQLite answers from the index unasked: {plan}");
    let mut after: Option<String> = None;
    let mut pages = 0;
    loop {
        let page = store.ready_page(None, after.as_deref(), Some(50)).unwrap();
        let (from_epic, from_seq) = match &after {
            Some(id) => {
                let key = store.tickets.read().unwrap().at[id].clone();
                (key.epic, i64::from(key.seq))
            }
            None => (String::new(), -1),
        };
        let indexed: Vec<String> = conn
            .prepare(
                "SELECT id FROM tickets INDEXED BY tickets_ready WHERE status = 'Ready' AND unsatisfied = 0
                     AND (epic, seq) > (?1, ?2) ORDER BY epic, seq LIMIT 50",
            )
            .unwrap()
            .query_map(rusqlite::params![from_epic, from_seq], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(page.tickets.iter().map(|t| t.id.clone()).collect::<Vec<_>>(), indexed, "after {after:?}");
        pages += 1;
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert!(pages > 3, "{pages} pages");
}

/// A walk of whole tickets (`ready_set {}` and `list { status }` without
/// `summary`) cuts its steps from the same kept set and ends where one
/// page of its limit ends.
#[tokio::test]
async fn a_walk_of_whole_tickets_answers_what_one_page_does() {
    let store = Store::new();
    store.seed_synthetic(SeedSpec { count: 900, seed: 4, into_existing: false }).unwrap();
    let cursor = rows(&store)[300].id.clone();
    for select in [Select::Ready { epic: None }, Select::List { epic: None, status: Some(Status::Ready), parent: None }] {
        for (after, limit) in [(None, None), (None, Some(150)), (Some(cursor.as_str()), Some(7)), (Some(cursor.as_str()), None)] {
            let mut walk = store.walk(select.clone(), after, limit);
            let mut ids = Vec::new();
            let end = loop {
                match walk.next(&store).await.unwrap() {
                    Step::Tickets(t) => ids.extend(t.into_iter().map(|t| t.id)),
                    Step::End { next, remaining, .. } => break (next, remaining),
                }
            };
            let page = match &select {
                Select::Ready { .. } => store.ready_page(None, after, limit),
                _ => store.page(PageQuery { status: Some(Status::Ready), after, limit, ..PageQuery::default() }),
            }
            .unwrap();
            assert_eq!((ids, end.0, end.1), answer(page), "{select:?} after {after:?} limit {limit:?}");
        }
    }
}
