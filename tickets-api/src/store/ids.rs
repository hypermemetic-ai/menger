use super::{OrderKey, Row, Rows};
use std::collections::{BTreeMap, HashMap};

pub(super) struct Ids {
    /// Each shortcode's highest sequence, in shortcode order — the
    /// store's order, byte-lexicographic on the epic. Nothing deletes a
    /// ticket, so this only ever rises, and plus one is the next id.
    /// SCALE-19: kept as the shortcode's [`Run`], which says beside it how
    /// many tickets the shortcode holds — whether the sequence alone
    /// describes it.
    runs: BTreeMap<String, Run>,
    /// The dangling forward references, kept from the end they name: an
    /// id that some ticket's `blocked_by` names and that is no ticket yet
    /// → those tickets, in the store's order. An entry leaves when its id
    /// is born; one that can never be (`RT-0`, a malformed id) stays, as
    /// it would in a scan.
    unborn: HashMap<String, BTreeMap<OrderKey, String>>,
}

/// SCALE-19: one shortcode's stretch of the store's order — what its
/// sequence says, and what it holds.
///
/// Ids are allocated highest-plus-one and nothing deletes, so every
/// shortcode the server made is *dense*: it holds each sequence from 1 to
/// its highest, its row k is `EPIC-k`, and "how many follow `EPIC-k`" is
/// `last − k` — arithmetic, not a walk. Only a database edited by hand can
/// break that (a hole; a sequence 0), and `held` is how the store knows it
/// was broken: kept beside the sequence by the same paths, so density is a
/// comparison, never a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    /// The lowest sequence held: 1, unless a hand edit wrote a 0.
    first: u32,
    /// The highest sequence held — SCALE-3's: plus one is the next id.
    last: u32,
    /// How many tickets the shortcode holds. Exact whatever the shape, so
    /// a whole shortcode is always counted by this, dense or not.
    held: u64,
}

impl Run {
    /// The run is every sequence from 1 to `last`: `held` distinct
    /// sequences, none below 1, none above `last` — so `held == last`
    /// leaves no room for a hole.
    fn dense(&self) -> bool {
        self.first >= 1 && self.held == u64::from(self.last)
    }
}

impl Ids {
    /// Both, from the tickets — at load, and after a seed.
    ///
    /// The sequences cost a descent per shortcode, not a step per ticket:
    /// the store's order makes a shortcode one contiguous run of keys, so
    /// its last key holds its highest sequence and the first key past
    /// `(epic, u32::MAX)` starts the next shortcode. The forward
    /// references are one pass over every ticket's `blocked_by`, each
    /// entry an existence check — the part that is O(N + edges), once.
    /// SCALE-19: and how many tickets each run holds is a step along it
    /// (a walk of keys already in order, no lookup per ticket) — also
    /// O(N), also once.
    pub(super) fn build(rows: Rows<'_>) -> Self {
        use std::ops::Bound::{Excluded, Included, Unbounded};
        let mut ids = Self {
            runs: BTreeMap::new(),
            unborn: HashMap::new(),
        };
        let mut next = rows.order.keys().next();
        while let Some(first) = next {
            let end = OrderKey {
                epic: first.epic.clone(),
                seq: u32::MAX,
            };
            let run = || rows.order.range::<OrderKey, _>((Included(first), Included(&end)));
            let (last, _) = run().next_back().expect("the run holds its own first key");
            ids.runs.insert(
                end.epic.clone(),
                Run {
                    first: first.seq,
                    last: last.seq,
                    held: run().count() as u64,
                },
            );
            next = rows
                .order
                .range::<OrderKey, _>((Excluded(&end), Unbounded))
                .next()
                .map(|(key, _)| key);
        }
        for t in rows.tickets() {
            ids.names(rows, t);
        }
        ids
    }

    /// The sequence `epic`'s next ticket gets: 1 for a shortcode that
    /// holds none.
    pub(super) fn next_seq(&self, epic: &str) -> u32 {
        self.runs.get(epic).map_or(1, |run| run.last + 1)
    }

    /// The tickets whose `blocked_by` already names `id`, in the store's
    /// order — a newborn's `unlocks` from birth.
    pub(super) fn waiting_on(&self, id: &str) -> Vec<String> {
        self.unborn
            .get(id)
            .map(|waiting| waiting.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Every shortcode that holds a ticket, with its highest sequence, in
    /// shortcode order.
    pub(super) fn last_seqs(&self) -> impl Iterator<Item = (&str, u32)> {
        self.runs.iter().map(|(epic, run)| (epic.as_str(), run.last))
    }

    /// SCALE-19: how many tickets `epic` holds — 0 for a shortcode that
    /// holds none.
    pub(super) fn held(&self, epic: &str) -> u64 {
        self.runs.get(epic).map_or(0, |run| run.held)
    }

    /// SCALE-19: how many tickets the shortcodes after `epic` hold, all
    /// together — the whole runs past it in the store's order. A step per
    /// shortcode, whatever each holds, and exact whether or not a run is
    /// dense: `held` is a count, not a sequence.
    pub(super) fn held_after(&self, epic: &str) -> u64 {
        use std::ops::Bound::{Excluded, Unbounded};
        self.runs
            .range::<str, _>((Excluded(epic), Unbounded))
            .map(|(_, run)| run.held)
            .sum()
    }

    /// SCALE-19: how many tickets of `key`'s shortcode follow `key` (a
    /// ticket in it) — `last − seq`, when the run is dense and so that is
    /// the count. `None` when it is not: the sequence no longer says, and
    /// the caller counts.
    pub(super) fn after_in_run(&self, key: &OrderKey) -> Option<u64> {
        let run = self.runs.get(&key.epic).filter(|run| run.dense())?;
        Some(u64::from(run.last.saturating_sub(key.seq)))
    }

    /// How many ids are named before they exist — for the startup log.
    pub(super) fn unborn_count(&self) -> usize {
        self.unborn.len()
    }

    /// A ticket is born (already in `rows`): its shortcode's sequence
    /// reaches it, the forward references to its id are resolved — they
    /// were read into its `unlocks` before the commit — and the ones it
    /// makes itself are kept. SCALE-19: and its shortcode holds one more.
    /// Born means new: `create` allocates past the highest sequence, so
    /// the newborn is never a ticket the run already counted.
    pub(super) fn born(&mut self, rows: Rows<'_>, t: &Row) {
        match self.runs.get_mut(&t.epic) {
            Some(run) => {
                run.first = run.first.min(t.seq);
                run.last = run.last.max(t.seq);
                run.held += 1;
            }
            None => {
                self.runs.insert(
                    t.epic.clone(),
                    Run {
                        first: t.seq,
                        last: t.seq,
                        held: 1,
                    },
                );
            }
        }
        self.unborn.remove(&t.id);
        self.names(rows, t);
    }

    /// Keeps each id `t` names in `blocked_by` that is no ticket. A ticket
    /// naming one id twice is one entry, as a scan finds it once.
    fn names(&mut self, rows: Rows<'_>, t: &Row) {
        for b in &t.blocked_by {
            if !rows.contains(b) {
                self.unborn
                    .entry(b.clone())
                    .or_default()
                    .insert(OrderKey::of(t), t.id.clone());
            }
        }
    }

    /// Every unborn id and who names it, sorted by id — for the tests'
    /// comparison with a scan.
    #[cfg(test)]
    pub(super) fn unborn(&self) -> BTreeMap<String, Vec<String>> {
        self.unborn
            .iter()
            .map(|(id, waiting)| (id.clone(), waiting.values().cloned().collect()))
            .collect()
    }

    /// Every shortcode's run as `(lowest sequence, highest, how many it
    /// holds, dense)` — for the tests' comparison with a scan (SCALE-19).
    #[cfg(test)]
    pub(super) fn runs(&self) -> BTreeMap<String, (u32, u32, u64, bool)> {
        self.runs
            .iter()
            .map(|(epic, run)| (epic.clone(), (run.first, run.last, run.held, run.dense())))
            .collect()
    }
}
