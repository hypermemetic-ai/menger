use super::stored::Changes;
use super::{satisfied, OrderKey, Row, Rows, Status};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Bound;

/// How many statuses there are — one count each per shortcode.
const STATUSES: usize = Status::ALL.len();

/// Where `status` sits in [`Status::ALL`], the order its counts are kept in.
fn slot(status: Status) -> usize {
    Status::ALL
        .iter()
        .position(|s| *s == status)
        .expect("every status is in Status::ALL")
}

/// MNG-4's rule, as a number: how many of the distinct ids `t` names in
/// `blocked_by` are not a `Complete` or `Superseded` ticket — an id that
/// names no ticket counts, as the scan has always counted it. `t` is in the
/// ready set when it is `Ready` and this is 0. Distinct, because a ticket
/// that names one blocker twice is unblocked by it once, and because that
/// blocker's `unlocks` names it once (`create` mirrors each blocker once).
pub(super) fn unsatisfied_of(rows: Rows<'_>, t: &Row) -> u32 {
    open_blockers(&t.blocked_by, |b| rows.get(b).map(|bt| bt.status))
}

/// The same rule over any way of finding a blocker's status — `None`: no
/// such ticket. A seed counts its tickets with it before their rows exist.
pub(super) fn open_blockers(blocked_by: &[String], status_of: impl Fn(&str) -> Option<Status>) -> u32 {
    let mut named: Vec<&str> = Vec::with_capacity(blocked_by.len());
    let mut open = 0;
    for b in blocked_by {
        if named.contains(&b.as_str()) {
            continue;
        }
        named.push(b);
        if !status_of(b).is_some_and(satisfied) {
            open += 1;
        }
    }
    open
}

pub(super) struct Ready {
    /// Each ticket with a blocker not done with, and how many it has
    /// ([`unsatisfied_of`]). A ticket with none is not here — most name no
    /// blocker at all.
    unsatisfied: HashMap<String, u32>,
    /// The ready set — `Ready`, and none unsatisfied — per shortcode, its
    /// members' sequences, ascending: the store's order, so a page is a
    /// range of it and a shortcode's count is its length. A `BTreeSet`, not
    /// a sorted vector as the children's are: a blocker that is completed
    /// can move a hundred thousand tickets in at once, anywhere in their
    /// shortcodes, and each is one descent rather than a shift of the rest.
    members: BTreeMap<String, BTreeSet<u32>>,
    /// Per shortcode, how many of its tickets hold each status, in
    /// [`Status::ALL`]'s order — what `list { status }` counts whole
    /// shortcodes with.
    statuses: BTreeMap<String, [u64; STATUSES]>,
}

/// SCALE-32: what one write does to the counts, the ready set and the
/// status counts — worked out before it commits ([`Ready::born`],
/// [`Ready::status_changed`]), stored by its transaction ([`Move::store`])
/// and taken by the cache after ([`Ready::moved`]): one value in two places.
#[derive(Default)]
pub(super) struct Move {
    /// Each ticket whose count moves, with the count it moves to.
    counts: Vec<(String, u32)>,
    /// Each ticket that joins (`true`) or leaves (`false`) the ready set.
    members: Vec<(OrderKey, bool)>,
    /// Each shortcode whose share of the ready set moves, with what it
    /// moves to — the stored count; the cache's is the set's length.
    ready: Vec<(String, u64)>,
    /// Each shortcode's count of a status that moves, with what it moves to.
    statuses: Vec<(String, Status, u64)>,
}

impl Move {
    /// The rows the write's own transaction writes for it: each count that
    /// moves (the ready set is the file's index over that column and the
    /// status — SQLite moves it with them), and each count of a shortcode.
    pub(super) fn store(&self, into: &mut Changes) {
        into.unsatisfied.extend(self.counts.iter().cloned());
        into.ready.extend(self.ready.iter().cloned());
        into.statuses
            .extend(self.statuses.iter().map(|(epic, s, n)| ((epic.clone(), s.as_str()), *n)));
    }

    /// How many tickets' counts move — for the tests' check that a write
    /// moved what a scan says it should, and no more.
    #[cfg(test)]
    pub(super) fn counts_moved(&self) -> usize {
        self.counts.len()
    }
}

/// The stored form of [`Ready`]: each count that is not 0, by ticket; each
/// shortcode's count of each status it holds; each shortcode's share of the
/// ready set.
pub(super) type Stored = (
    BTreeMap<String, u32>,
    BTreeMap<(String, &'static str), u64>,
    BTreeMap<String, u64>,
);

impl Ready {
    /// All three, from the tickets — at a build, after a seed, and for
    /// `--verify-derived` to compare with: a pass over every ticket, each
    /// `blocked_by` entry a lookup. O(N + edges), once.
    pub(super) fn build(rows: Rows<'_>) -> Self {
        let mut r = Self {
            unsatisfied: HashMap::new(),
            members: BTreeMap::new(),
            statuses: BTreeMap::new(),
        };
        for t in rows.tickets() {
            let open = unsatisfied_of(rows, t);
            if open > 0 {
                r.unsatisfied.insert(t.id.clone(), open);
            }
            if t.status == Status::Ready && open == 0 {
                r.members.entry(t.epic.clone()).or_default().insert(t.seq);
            }
            r.statuses.entry(t.epic.clone()).or_insert([0; STATUSES])[slot(t.status)] += 1;
        }
        r
    }

    /// SCALE-32: from what is stored — at start, instead of [`Self::build`]:
    /// the counts as each ticket's row carries them, the ready set as the
    /// file's index holds it, and the status counts' rows.
    pub(super) fn from_stored(
        unsatisfied: HashMap<String, u32>,
        members: BTreeMap<String, BTreeSet<u32>>,
        statuses: BTreeMap<(String, &'static str), u64>,
    ) -> Self {
        let mut by_epic: BTreeMap<String, [u64; STATUSES]> = BTreeMap::new();
        for ((epic, status), held) in statuses {
            let status: Status = status.parse().expect("parsed from a status when it was read");
            by_epic.entry(epic).or_insert([0; STATUSES])[slot(status)] = held;
        }
        Self {
            unsatisfied,
            members,
            statuses: by_epic,
        }
    }

    /// As stored ([`Stored`]).
    pub(super) fn stored(&self) -> Stored {
        let unsatisfied = self.unsatisfied.iter().map(|(id, n)| (id.clone(), *n)).collect();
        let statuses = self
            .statuses
            .iter()
            .flat_map(|(epic, held)| {
                Status::ALL
                    .iter()
                    .zip(held)
                    .filter(|(_, n)| **n > 0)
                    .map(move |(s, n)| ((epic.clone(), s.as_str()), *n))
            })
            .collect();
        let ready = self.members.iter().map(|(epic, seqs)| (epic.clone(), seqs.len() as u64)).collect();
        (unsatisfied, statuses, ready)
    }

    /// The ready set, per shortcode — what `--verify-derived` and the tests
    /// compare with the file's index.
    pub(super) fn members(&self) -> &BTreeMap<String, BTreeSet<u32>> {
        &self.members
    }

    /// How many tickets the ready set holds — for the logs and the verdict.
    pub(super) fn size(&self) -> usize {
        self.members.values().map(BTreeSet::len).sum()
    }

    /// How many tickets have a blocker not done with.
    pub(super) fn blocked(&self) -> usize {
        self.unsatisfied.len()
    }

    /// How many shortcodes have their status counts kept.
    pub(super) fn shortcodes(&self) -> usize {
        self.statuses.len()
    }

    fn count(&self, id: &str) -> u32 {
        self.unsatisfied.get(id).copied().unwrap_or(0)
    }

    fn contains(&self, key: &OrderKey) -> bool {
        self.members.get(&key.epic).is_some_and(|seqs| seqs.contains(&key.seq))
    }

    /// A ticket is born (`rows` hold it): its own count, from its blockers
    /// as they stand — an id not born yet counts — and its status counted.
    /// Born satisfied, it would also take one from each ticket that named it
    /// before it existed (`unlocks` at birth is exactly those); `create`
    /// births every ticket `Pending`, which is not, so the tickets that were
    /// waiting on its id wait on it still — their counts do not move (the
    /// tests hold this to the scan rather than assume it).
    pub(super) fn born(&self, rows: Rows<'_>, t: &Row) -> Move {
        let mut m = Move::default();
        let open = unsatisfied_of(rows, t);
        if open > 0 {
            m.counts.push((t.id.clone(), open));
        }
        self.restatus(t, None, &mut m);
        let mut touched: Vec<(&Row, u32)> = vec![(t, open)];
        if satisfied(t.status) {
            self.fan_out(rows, t, true, &mut m, &mut touched);
        }
        self.settle(&touched, &mut m);
        m
    }

    /// `t`'s status changed from `previous` (`rows` hold it as changed): its
    /// status counted where it now is; and when the change crosses the line
    /// MNG-4's rule draws — into or out of `Complete`/`Superseded` — every
    /// ticket that names it moves its count by one, found from `t`'s
    /// `unlocks`, the mirror the store keeps of the tickets that name it
    /// (MNG-4; never a scan). A ticket's own count never moves with its own
    /// status: its blockers did not change. Then whoever's status or count
    /// moved joins or leaves the ready set. The fan-out is the cost: a
    /// crossing of a ticket that blocks k others writes k counts.
    pub(super) fn status_changed(&self, rows: Rows<'_>, t: &Row, previous: Status) -> Move {
        let mut m = Move::default();
        if previous == t.status {
            return m;
        }
        self.restatus(t, Some(previous), &mut m);
        let mut touched: Vec<(&Row, u32)> = Vec::new();
        if satisfied(previous) != satisfied(t.status) {
            self.fan_out(rows, t, satisfied(t.status), &mut m, &mut touched);
        }
        // `t` named itself (a hand edit — `create` refuses it) and is
        // already here, with the count that moved.
        if !touched.iter().any(|(r, _)| r.id == t.id) {
            touched.push((t, self.count(&t.id)));
        }
        self.settle(&touched, &mut m);
        m
    }

    /// `t`'s status counted in its shortcode — moved from `previous`, or
    /// new.
    fn restatus(&self, t: &Row, previous: Option<Status>, m: &mut Move) {
        let mut held = self.statuses.get(&t.epic).copied().unwrap_or([0; STATUSES]);
        if let Some(p) = previous {
            held[slot(p)] = held[slot(p)].saturating_sub(1);
            m.statuses.push((t.epic.clone(), p, held[slot(p)]));
        }
        held[slot(t.status)] += 1;
        m.statuses.push((t.epic.clone(), t.status, held[slot(t.status)]));
    }

    /// Each ticket in `t`'s `unlocks` that names it gains one unsatisfied
    /// blocker, or loses one (`done`). An entry twice is one ticket; one
    /// that names no ticket, or a ticket that does not name `t` (only a hand
    /// edit leaves either), is not counted — its count does not hang on `t`.
    fn fan_out<'a>(&self, rows: Rows<'a>, t: &Row, done: bool, m: &mut Move, touched: &mut Vec<(&'a Row, u32)>) {
        let mut seen: HashSet<&str> = HashSet::with_capacity(t.unlocks.len());
        for id in &t.unlocks {
            if !seen.insert(id) {
                continue;
            }
            let Some(r) = rows.get(id) else { continue };
            if !r.blocked_by.contains(&t.id) {
                continue;
            }
            let was = self.count(id);
            let now = if done { was.saturating_sub(1) } else { was + 1 };
            m.counts.push((id.clone(), now));
            touched.push((r, now));
        }
    }

    /// Whoever's status or count moved, as the write leaves them: each joins
    /// the ready set or leaves it where that is not where it stands; then
    /// each shortcode whose share moved, with its new count.
    fn settle(&self, touched: &[(&Row, u32)], m: &mut Move) {
        let mut shares: BTreeMap<&str, u64> = BTreeMap::new();
        for (r, open) in touched {
            let key = OrderKey::of(r);
            let now = r.status == Status::Ready && *open == 0;
            if now == self.contains(&key) {
                continue;
            }
            let share = shares
                .entry(r.epic.as_str())
                .or_insert_with(|| self.members.get(&r.epic).map_or(0, |s| s.len() as u64));
            *share = if now { *share + 1 } else { share.saturating_sub(1) };
            m.members.push((key, now));
        }
        m.ready.extend(shares.into_iter().map(|(epic, n)| (epic.to_string(), n)));
    }

    /// …and once it has committed, the cache takes it.
    pub(super) fn moved(&mut self, m: Move) {
        for (id, n) in m.counts {
            if n == 0 {
                self.unsatisfied.remove(&id);
            } else {
                self.unsatisfied.insert(id, n);
            }
        }
        for (key, join) in m.members {
            if join {
                self.members.entry(key.epic).or_default().insert(key.seq);
            } else if let Some(seqs) = self.members.get_mut(&key.epic) {
                seqs.remove(&key.seq);
                if seqs.is_empty() {
                    self.members.remove(&key.epic);
                }
            }
        }
        for (epic, status, n) in m.statuses {
            self.statuses.entry(epic).or_insert([0; STATUSES])[slot(status)] = n;
        }
    }

    /// The shortcodes whose members can lie inside `(start, end)`, each with
    /// the stretch of its sequences that does, as an inclusive range — the
    /// epic bounds are the keys' (a key bound excluded still admits the rest
    /// of its shortcode). A span never ends before it starts (`Span::of`);
    /// were one asked for, it holds nothing — never a panic under the read
    /// lock.
    fn cut<'a>(
        &'a self,
        start: Bound<&'a OrderKey>,
        end: Bound<&'a OrderKey>,
    ) -> impl Iterator<Item = (&'a str, &'a BTreeSet<u32>, u32, u32)> + 'a {
        use Bound::{Excluded, Included, Unbounded};
        let epic = |b: Bound<&'a OrderKey>| match b {
            Unbounded => Unbounded,
            Included(k) | Excluded(k) => Included(k.epic.as_str()),
        };
        let inverted = matches!(
            (start, end),
            (Included(a) | Excluded(a), Included(b) | Excluded(b)) if a.epic > b.epic
        );
        let epics = (!inverted).then(|| self.members.range::<str, _>((epic(start), epic(end))));
        epics.into_iter().flatten().filter_map(move |(e, seqs)| {
            let from = match start {
                Included(k) if k.epic == *e => Some(k.seq),
                Excluded(k) if k.epic == *e => k.seq.checked_add(1),
                _ => Some(0),
            }?;
            let to = match end {
                Included(k) if k.epic == *e => Some(k.seq),
                Excluded(k) if k.epic == *e => k.seq.checked_sub(1),
                _ => Some(u32::MAX),
            }?;
            (from <= to).then_some((e.as_str(), seqs, from, to))
        })
    }

    /// The ready set inside `(start, end)`, in the store's order — what a
    /// page of `ready_set` walks: a descent to the cursor, then the page.
    pub(super) fn range<'a>(
        &'a self,
        start: Bound<&'a OrderKey>,
        end: Bound<&'a OrderKey>,
    ) -> impl Iterator<Item = OrderKey> + 'a {
        self.cut(start, end).flat_map(|(epic, seqs, from, to)| {
            seqs.range(from..=to).map(move |&seq| OrderKey {
                epic: epic.to_string(),
                seq,
            })
        })
    }

    /// How many of the ready set lie inside `(start, end)`: a whole
    /// shortcode is its length, and only the one a bound cuts into is
    /// counted — the members of it past the cursor, never the rest of the
    /// store. What makes `remaining` on a page of `ready_set` cost at most
    /// the ready members of one shortcode.
    pub(super) fn len(&self, start: Bound<&OrderKey>, end: Bound<&OrderKey>) -> u64 {
        self.cut(start, end)
            .map(|(_, seqs, from, to)| match (from, to) {
                (0, u32::MAX) => seqs.len() as u64,
                _ => seqs.range(from..=to).count() as u64,
            })
            .sum()
    }

    /// How many tickets of `epic` hold `status`.
    pub(super) fn held(&self, epic: &str, status: Status) -> u64 {
        self.statuses.get(epic).map_or(0, |held| held[slot(status)])
    }

    /// How many tickets hold `status` in the shortcodes after `epic` — or
    /// in every shortcode, with none. A step per shortcode, whatever each
    /// holds.
    pub(super) fn held_after(&self, epic: Option<&str>, status: Status) -> u64 {
        use Bound::{Excluded, Unbounded};
        let from = epic.map_or(Unbounded, Excluded);
        self.statuses
            .range::<str, _>((from, Unbounded))
            .map(|(_, held)| held[slot(status)])
            .sum()
    }

    /// Every ticket's count that is not 0, sorted by id — for the tests'
    /// comparison with a scan.
    #[cfg(test)]
    pub(super) fn counts(&self) -> BTreeMap<String, u32> {
        self.unsatisfied.iter().map(|(id, n)| (id.clone(), *n)).collect()
    }
}
