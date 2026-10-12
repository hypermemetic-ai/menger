//! FILTER-4, acceptance 2: the answers equal a scan. For stores built by
//! create, by seed and from the fixtures, and after each of hundreds of
//! random writes of every kind, random filters — `counts`, every filtered
//! page after random cursors (rows, `next`, `remaining`, `first`), every
//! page at random positions, with `list`'s old parameters beside them —
//! equal the same filter tested on every row. In the cache, and after a
//! reopen, which reads the stored form.
//!
//! The filters here are [`Lens`]es: a judge of (shortcode, cell) and the
//! shortcodes it names. [`random_lens`] makes them from a small random
//! formula over every cell field and `in`; `tests_language` (once the
//! language is in) makes them from the language's own filters.

use super::cells::Key;
use super::tests_cells::scanned;
use super::tests_ready::{random_write, rows, Rng, Tally};
use super::*;

/// One page's answer: the ids, `next`, `remaining`, `first`.
pub(super) type Answer = (Vec<String>, Option<String>, u64, Option<u64>);

fn answer(page: Page) -> Answer {
    (page.tickets.into_iter().map(|t| t.id).collect(), page.next, page.remaining, page.first)
}

/// A random formula over a cell and a shortcode — every field a cell has,
/// `in`, and `and` / `or` / `not` nested — as a judge, with the shortcodes
/// it names.
#[derive(Debug, Clone)]
enum Formula {
    Status(Status),
    Kind(Kind),
    Severity(Option<Severity>),
    Blocked,
    Commits,
    /// FILTER-21: `menger doc`.
    Menger,
    In(String),
    Not(Box<Formula>),
    And(Vec<Formula>),
    Or(Vec<Formula>),
}

impl Formula {
    fn holds(&self, epic: &str, k: Key) -> bool {
        match self {
            Self::Status(s) => k.status() == *s,
            Self::Kind(x) => k.kind() == *x,
            Self::Severity(s) => k.severity() == *s,
            Self::Blocked => k.blocked(),
            Self::Commits => k.commits(),
            Self::Menger => k.menger(),
            Self::In(e) => e == epic,
            Self::Not(f) => !f.holds(epic, k),
            Self::And(fs) => fs.iter().all(|f| f.holds(epic, k)),
            Self::Or(fs) => fs.iter().any(|f| f.holds(epic, k)),
        }
    }

    fn named(&self, out: &mut Vec<String>) {
        match self {
            Self::In(e) => out.push(e.clone()),
            Self::Not(f) => f.named(out),
            Self::And(fs) | Self::Or(fs) => fs.iter().for_each(|f| f.named(out)),
            _ => {}
        }
    }

    fn random(rng: &mut Rng, epics: &[String], depth: usize) -> Self {
        let leaf = depth == 0 || rng.below(3) == 0;
        if leaf {
            return match rng.below(8) {
                0 => Self::Status(Status::ALL[rng.below(Status::ALL.len())]),
                1 => Self::Kind(Kind::ALL[rng.below(Kind::ALL.len())]),
                2 => Self::Severity(super::cells::SEVERITIES[rng.below(5)]),
                3 => Self::Blocked,
                4 => Self::Commits,
                5 => Self::Menger,
                _ if !epics.is_empty() => Self::In(epics[rng.below(epics.len())].clone()),
                _ => Self::Blocked,
            };
        }
        let n = 1 + rng.below(3);
        let parts = (0..n).map(|_| Self::random(rng, epics, depth - 1)).collect();
        match rng.below(3) {
            0 => Self::And(parts),
            1 => Self::Or(parts),
            _ => Self::Not(Box::new(Self::random(rng, epics, depth - 1))),
        }
    }
}

/// A random lens, and the same judge for the oracle.
fn random_lens(rng: &mut Rng, epics: &[String]) -> (Lens, Formula) {
    let f = if rng.below(8) == 0 { Formula::And(vec![]) } else { Formula::random(rng, epics, 3) };
    let mut named = Vec::new();
    f.named(&mut named);
    let judge = f.clone();
    (Lens::new(format!("{f:?}"), named, move |e, k| judge.holds(e, k)), f)
}

/// The oracle's judge: a row, and its cell by a scan (what `blocked`
/// reads), tested.
pub(super) type RowJudge<'a> = &'a dyn Fn(&Row, Key) -> bool;

/// The store as a plain list, and each row's cell by a scan: the oracle's
/// whole input.
pub(super) struct Plain {
    pub(super) rows: Vec<Row>,
    pub(super) cells: Vec<Key>,
    at: HashMap<String, usize>,
    /// FILTER-13: each row's lineage, as FILTER-10's `under` reads it: its
    /// id, its strict ancestors' ids (the lineage's rule: each once, the
    /// chain ending at an id that names no ticket, or where it meets one
    /// met already or the row itself), its shortcode, and the shortcodes
    /// hosting it (each shortcode's host as the roll-ups settle it, MNG-14).
    pub(super) lineage: Vec<Vec<String>>,
}

impl Plain {
    pub(super) fn of(store: &Store) -> Self {
        let rows = rows(store);
        let asking = super::tests_cells::asking(store);
        let by = scanned(&rows, &asking);
        let cells = rows.iter().map(|t| by[&OrderKey::of(t)]).collect();
        let at = rows.iter().enumerate().map(|(i, t)| (t.id.clone(), i)).collect();
        let by_id: HashMap<&str, &Row> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
        let hosts: HashMap<String, Vec<String>> = {
            let cache = store.tickets.read().unwrap();
            let epics: std::collections::BTreeSet<&str> = rows.iter().map(|t| t.epic.as_str()).collect();
            epics
                .into_iter()
                .map(|e| {
                    let mut chain: Vec<String> = Vec::new();
                    let mut cur = e.to_string();
                    while let Some(h) = cache.rollups.host(&cur) {
                        let Some(he) = by_id.get(h).map(|r| r.epic.clone()) else { break };
                        if he == e || chain.contains(&he) {
                            break;
                        }
                        chain.push(he.clone());
                        cur = he;
                    }
                    (e.to_string(), chain)
                })
                .collect()
        };
        let lineage = rows
            .iter()
            .map(|t| {
                let mut l = vec![t.id.clone()];
                l.extend(super::tests_tree::ancestors_of(&by_id, t));
                l.push(t.epic.clone());
                l.extend(hosts[&t.epic].iter().cloned());
                l
            })
            .collect();
        Self { rows, cells, at, lineage }
    }

    /// FILTER-13: what the reference evaluator reads of row `t` (cell `k`) —
    /// its whole lineage included.
    pub(super) fn facts(&self, t: &Row, k: Key) -> crate::filter::Facts {
        crate::filter::Facts {
            asking: k.asking(),
            labels: t.labels.clone(),
            id: t.id.clone(),
            epic: t.epic.clone(),
            title: t.title.clone(),
            status: t.status,
            kind: t.kind,
            severity: t.severity,
            blocked: k.blocked(),
            has_commits: !t.commits.is_empty(),
            menger_doc: t.path.as_deref().is_some_and(|p| p.starts_with("_MENGER/")),
            lineage: self.at.get(&t.id).map_or_else(|| vec![t.id.clone(), t.epic.clone()], |i| self.lineage[*i].clone()),
        }
    }

    /// The matches of the filter, `list`'s parameters beside it, by testing
    /// every row.
    pub(super) fn matches(&self, judge: RowJudge<'_>, epic: Option<&str>, status: Option<Status>, parent: Option<&str>) -> Vec<usize> {
        (0..self.rows.len())
            .filter(|i| {
                let t = &self.rows[*i];
                judge(t, self.cells[*i])
                    && epic.is_none_or(|e| t.epic.eq_ignore_ascii_case(e) && t.epic == t.epic.to_ascii_uppercase())
                    && status.is_none_or(|s| t.status == s)
                    && parent.is_none_or(|p| t.parent.as_deref() == Some(p.to_ascii_uppercase().as_str()))
            })
            .collect()
    }

    /// `counts`: each shortcode with a match, and how many.
    pub(super) fn counts(&self, judge: RowJudge<'_>) -> Vec<(String, u64)> {
        let mut out: Vec<(String, u64)> = Vec::new();
        for i in self.matches(judge, None, None, None) {
            let e = &self.rows[i].epic;
            match out.last_mut() {
                Some((last, n)) if last == e => *n += 1,
                _ => out.push((e.clone(), 1)),
            }
        }
        out
    }

    /// A filtered page by the rule: after a cursor (a position: the first
    /// match past it) or at a position, `limit` of them; `first` where it
    /// starts; `remaining` what is left; `next` the last row, or for an
    /// empty page the cursor, or the match before the position.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn page(
        &self,
        judge: RowJudge<'_>,
        epic: Option<&str>,
        status: Option<Status>,
        parent: Option<&str>,
        after: Option<&str>,
        at: Option<u64>,
        limit: Option<u32>,
    ) -> Result<Answer, String> {
        if after.is_some() && at.is_some() {
            return Err("both".into());
        }
        let m = self.matches(judge, epic, status, parent);
        let first = match (after, at) {
            (Some(id), _) => {
                let id = id.to_ascii_uppercase();
                let Some(c) = self.at.get(&id) else { return Err(format!("after: no ticket {id}")) };
                let key = OrderKey::of(&self.rows[*c]);
                m.iter().filter(|i| OrderKey::of(&self.rows[**i]) <= key).count() as u64
            }
            (None, Some(p)) => p,
            (None, None) => 0,
        };
        let from = (first as usize).min(m.len());
        let to = limit.map_or(m.len(), |n| (from + n as usize).min(m.len()));
        let page: Vec<String> = m[from..to].iter().map(|i| self.rows[*i].id.clone()).collect();
        let remaining = (m.len() as u64).saturating_sub(first).saturating_sub(page.len() as u64);
        let next = if remaining == 0 {
            None
        } else {
            page.last().cloned().or_else(|| match after {
                Some(id) => Some(id.to_ascii_uppercase()),
                None if first == 0 => None,
                None => Some(self.rows[m[first as usize - 1]].id.clone()),
            })
        };
        Ok((page, next, remaining, Some(first)))
    }
}

/// The store's answer.
#[allow(clippy::too_many_arguments)]
fn answered(store: &Store, lens: &Lens, epic: Option<&str>, status: Option<Status>, parent: Option<&str>, after: Option<&str>, at: Option<u64>, limit: Option<u32>) -> Result<Answer, String> {
    answered_keeping(store, lens, epic, status, parent, after, at, limit, None)
}

/// FILTER-24: the store's answer with `keep`.
#[allow(clippy::too_many_arguments)]
fn answered_keeping(
    store: &Store,
    lens: &Lens,
    epic: Option<&str>,
    status: Option<Status>,
    parent: Option<&str>,
    after: Option<&str>,
    at: Option<u64>,
    limit: Option<u32>,
    keep: Option<&str>,
) -> Result<Answer, String> {
    let q = Filtered { lens: lens.clone(), epic: epic.map(String::from), status, parent: parent.map(String::from), keep: keep.map(String::from) };
    store.filtered_page(&q, after, at, limit).map(answer).map_err(|e| e.to_string())
}

/// For one lens: `counts`, and pages after a sample of cursors and at a
/// sample of positions, with `list`'s parameters beside it or not; then a
/// whole walk by `next`, and one by `at`.
#[track_caller]
pub(super) fn assert_lens_is_a_scan(store: &Store, plain: &Plain, lens: &Lens, judge: RowJudge<'_>, rng: &mut Rng, what: &str) {
    let what = format!("{what}: {lens:?}");
    let counts = plain.counts(judge);
    assert_eq!(store.counts(lens), counts, "{what}: counts");
    let total: u64 = counts.iter().map(|(_, n)| n).sum();
    let ids: Vec<&str> = plain.rows.iter().map(|t| t.id.as_str()).collect();
    let pick = |rng: &mut Rng| -> Option<String> { (!ids.is_empty()).then(|| ids[rng.below(ids.len())].to_string()) };
    let epics: Vec<String> = counts.iter().map(|(e, _)| e.clone()).collect();
    for _ in 0..6 {
        let epic = match rng.below(5) {
            0 if !epics.is_empty() => Some(epics[rng.below(epics.len())].clone()),
            1 => Some("NOTHING".to_string()),
            2 if !epics.is_empty() => Some(epics[rng.below(epics.len())].to_lowercase()),
            _ => None,
        };
        let status = (rng.below(4) == 0).then(|| Status::ALL[rng.below(Status::ALL.len())]);
        let parent = match rng.below(6) {
            0 => pick(rng),
            1 => plain.rows.iter().find(|t| t.seq == 1).map(|t| t.id.clone()),
            _ => None,
        };
        let limit = [None, Some(0), Some(1), Some(3), Some(200)][rng.below(5)];
        let after = match rng.below(5) {
            0 => None,
            1 => Some("NO-SUCH-1".to_string()),
            2 => pick(rng).map(|id| id.to_lowercase()),
            _ => pick(rng),
        };
        let a = (epic.as_deref(), status, parent.as_deref());
        let want = plain.page(judge, a.0, a.1, a.2, after.as_deref(), None, limit);
        let got = answered(store, lens, a.0, a.1, a.2, after.as_deref(), None, limit);
        assert_eq!(got, want, "{what}: {a:?} after {after:?} limit {limit:?}");
        let at = match rng.below(4) {
            0 => 0,
            1 => total + rng.below(3) as u64,
            _ => rng.below(total as usize + 1) as u64,
        };
        let want = plain.page(judge, a.0, a.1, a.2, None, Some(at), limit);
        let got = answered(store, lens, a.0, a.1, a.2, None, Some(at), limit);
        assert_eq!(got, want, "{what}: {a:?} at {at} limit {limit:?}");
        // FILTER-24: and keeping a ticket — a match whatever the filter
        // says of it, inside the sugar — by a generator of its own, so the
        // reads above see what they saw before it.
        let mut krng = Rng(rng.0 ^ 0xF1_17E2_0024);
        if let Some(keep) = pick(&mut krng) {
            let kept: RowJudge<'_> = &|t: &Row, k: Key| judge(t, k) || t.id == keep;
            let at = krng.below(total as usize + 2) as u64;
            let want = plain.page(kept, a.0, a.1, a.2, after.as_deref(), None, limit);
            let got = answered_keeping(store, lens, a.0, a.1, a.2, after.as_deref(), None, limit, Some(&keep));
            assert_eq!(got, want, "{what}: {a:?} keeping {keep} after {after:?} limit {limit:?}");
            let want = plain.page(kept, a.0, a.1, a.2, None, Some(at), limit);
            let got = answered_keeping(store, lens, a.0, a.1, a.2, None, Some(at), limit, Some(&keep));
            assert_eq!(got, want, "{what}: {a:?} keeping {keep} at {at} limit {limit:?}");
            assert_eq!(store.counts_keeping(lens, Some(&keep)), plain.counts(kept), "{what}: counts keeping {keep}");
        }
    }
    if let Some(id) = ids.first() {
        let both = answered(store, lens, None, None, None, Some(id), Some(0), None).unwrap_err();
        assert!(both.contains("not both"), "{what}: {both}");
    }
    // A walk by `next`, and one by `at`, visit every match once.
    let limit = 1 + rng.below(40) as u32;
    let (mut after, mut seen, mut pages) = (None::<String>, Vec::new(), 0);
    // A walk by `next` ends early only at an id no cursor can name again
    // (lower case: only a hand edit writes one — SCALE-19), as before.
    let nameable = ids.iter().all(|id| *id == id.to_ascii_uppercase());
    loop {
        let Ok((page, next, _, first)) = answered(store, lens, None, None, None, after.as_deref(), None, Some(limit)) else {
            assert!(!nameable, "{what}: a cursor the page handed back names a ticket");
            break;
        };
        assert_eq!(first, Some(seen.len() as u64), "{what}: each page starts where the last ended");
        seen.extend(page);
        pages += 1;
        match next {
            Some(n) => after = Some(n),
            None => break,
        }
        assert!(pages < 10_000);
    }
    if nameable {
        assert_eq!(seen.len() as u64, total, "{what}: a walk by next sees every match");
    }
    let mut by_at = Vec::new();
    while (by_at.len() as u64) < total {
        let (page, _, _, _) = answered(store, lens, None, None, None, None, Some(by_at.len() as u64), Some(limit)).unwrap();
        assert!(!page.is_empty());
        by_at.extend(page);
    }
    if nameable {
        assert_eq!(by_at, seen, "{what}: a walk by at is the walk by next");
    }
}

/// `writes` random writes; every `every` of them, a handful of random lenses
/// held to the scan.
async fn walk(store: Store, path: &Path, seed: u64, writes: usize, every: usize, epics: &[&str]) -> Store {
    let mut rng = Rng(seed);
    // FILTER-21: board notes among the docs.
    let mut tally = Tally::asking_and_labelling();
    for step in 0..writes {
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        if step % every == every - 1 {
            let plain = Plain::of(&store);
            let held: Vec<String> = store.extents().into_iter().map(|(e, _)| e).chain(["NONE".into()]).collect();
            for _ in 0..4 {
                let (lens, f) = random_lens(&mut rng, &held);
                assert_lens_is_a_scan(&store, &plain, &lens, &|t: &Row, k| f.holds(&t.epic, k), &mut rng, &format!("step {step}: {what}"));
            }
        }
    }
    drop(store);
    let store = Store::open(path).unwrap();
    let plain = Plain::of(&store);
    let held: Vec<String> = store.extents().into_iter().map(|(e, _)| e).collect();
    for _ in 0..8 {
        let (lens, f) = random_lens(&mut rng, &held);
        assert_lens_is_a_scan(&store, &plain, &lens, &|t: &Row, k| f.holds(&t.epic, k), &mut rng, "the reopen");
    }
    store
}

#[tokio::test]
async fn filtered_answers_are_a_scan_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    let store = walk(store, &path, 0xF117_E4A2, 400, 10, &["AA", "MAP", "SUB", "ZED"]).await;
    // FILTER-21: the lenses above judged board notes too — the walk filed some.
    assert!(Plain::of(&store).cells.iter().any(|k| k.menger()), "the walk files menger docs");
}

#[tokio::test]
async fn filtered_answers_are_a_scan_on_a_seeded_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seeded.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 2_000, seed: 41, into_existing: false }).unwrap();
    let epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).take(5).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    walk(store, &path, 0x5EED_F4A2, 200, 20, &epics).await;
}

#[tokio::test]
async fn filtered_answers_are_a_scan_on_each_fixture() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        let store = Store::open(&path).unwrap();
        let mut epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        walk(store, &path, 0xF1A2 + i as u64, 100, 10, &epics).await;
    }
}

/// `watch { filter }`'s membership, for one write after another: what the
/// filtered channel's moves say entered and left is exactly what a scan
/// before and after the write says — and past the threshold, the counts.
#[tokio::test]
async fn membership_is_the_scans_difference() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("member.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 600, seed: 7, into_existing: false }).unwrap();
    let epics: Vec<String> = store.extents().into_iter().map(|(e, _)| e).take(4).collect();
    let mut rng = Rng(0x3E3B);
    let lenses: Vec<(Lens, Formula)> = (0..6).map(|_| random_lens(&mut rng, &epics)).collect();
    let mut watchers: Vec<Watcher> = lenses.iter().map(|(l, _)| store.watcher(l.clone())).collect();
    let mut rx = store.subscribe_filtered();
    let epic_refs: Vec<&str> = epics.iter().map(String::as_str).collect();
    // FILTER-21: board notes among the docs.
    let mut tally = Tally::asking_and_labelling();
    let mut moved = 0;
    for step in 0..200 {
        let before = Plain::of(&store);
        let what = random_write(&store, &mut rng, &epic_refs, &mut tally).await;
        let after = Plain::of(&store);
        let mut writes = Vec::new();
        while let Ok(change) = rx.try_recv() {
            if let Change::CellsMoved { moves } = change {
                writes.push(moves);
            }
        }
        assert!(writes.len() <= 1, "one CellsMoved a write");
        for ((lens, f), watcher) in lenses.iter().zip(&mut watchers) {
            let judge = |t: &Row, k: Key| f.holds(&t.epic, k);
            let inside = |p: &Plain| -> BTreeMap<String, (String, u32)> {
                p.matches(&judge, None, None, None).into_iter().map(|i| (p.rows[i].id.clone(), (p.rows[i].epic.clone(), p.rows[i].seq))).collect()
            };
            let (was, now) = (inside(&before), inside(&after));
            let mut want: Vec<(OrderKey, Membership)> = Vec::new();
            for (id, (epic, seq)) in &now {
                if !was.contains_key(id) {
                    want.push((OrderKey { epic: epic.clone(), seq: *seq }, Membership::Entered { id: id.clone(), epic: epic.clone(), seq: *seq }));
                }
            }
            for (id, (epic, seq)) in &was {
                if !now.contains_key(id) {
                    want.push((OrderKey { epic: epic.clone(), seq: *seq }, Membership::Left { id: id.clone(), epic: epic.clone(), seq: *seq }));
                }
            }
            want.sort_by(|a, b| a.0.cmp(&b.0));
            let want: Vec<Membership> = want.into_iter().map(|(_, m)| m).collect();
            let got: Vec<Membership> = writes.iter().flat_map(|m| watcher.membership(&store, m, None)).collect();
            assert_eq!(got, want, "step {step}: {what}: {lens:?}");
            moved += got.len();
        }
    }
    assert!(moved > 50, "{moved} membership moves");
}
