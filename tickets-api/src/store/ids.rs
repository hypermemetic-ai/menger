use super::stored::{Changes, RefKey, RunRow};
use super::{OrderKey, Row, Rows};
use crate::address::{ShortcodeId, TicketId, TicketName};
use std::collections::{BTreeMap, HashMap};

pub(super) struct Ids {
    /// Each shortcode's highest sequence, in shortcode order — the
    /// store's order, byte-lexicographic on the epic. Nothing deletes a
    /// ticket, so this only ever rises, and plus one is the next id.
    /// SCALE-19: kept as the shortcode's [`Run`], which says beside it how
    /// many tickets the shortcode holds — whether the sequence alone
    /// describes it.
    runs: BTreeMap<ShortcodeId, Run>,
    /// The dangling forward references, kept from the end they name: an
    /// id that some ticket's `blocked_by` names and that is no ticket yet
    /// → those tickets, in the store's order. An entry leaves when its id
    /// is born; one that can never be (`RT-0`, a malformed id) stays, as
    /// it would in a scan.
    unborn: HashMap<TicketName, BTreeMap<OrderKey, TicketId>>,
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
                shortcode: first.shortcode.clone(),
                seq: u32::MAX,
            };
            let run = || rows.order.range::<OrderKey, _>((Included(first), Included(&end)));
            let (last, _) = run().next_back().expect("the run holds its own first key");
            ids.runs.insert(
                end.shortcode.clone(),
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
    pub(super) fn next_seq(&self, epic: &ShortcodeId) -> u32 {
        self.runs.get(epic).map_or(1, |run| run.last + 1)
    }

    /// The tickets whose `blocked_by` already names `id`, in the store's
    /// order — a newborn's `unlocks` from birth.
    pub(super) fn waiting_on(&self, id: &str) -> Vec<TicketName> {
        self.unborn
            .get(id)
            .map(|waiting| waiting.values().map(TicketName::from).collect())
            .unwrap_or_default()
    }

    /// Every shortcode that holds a ticket, with its highest sequence, in
    /// shortcode order.
    pub(super) fn last_seqs(&self) -> impl Iterator<Item = (&ShortcodeId, u32)> {
        self.runs.iter().map(|(epic, run)| (epic, run.last))
    }

    /// SCALE-19: how many tickets `epic` holds — 0 for a shortcode that
    /// holds none.
    pub(super) fn held(&self, epic: &ShortcodeId) -> u64 {
        self.runs.get(epic).map_or(0, |run| run.held)
    }

    /// SCALE-19: how many tickets the shortcodes after `epic` hold, all
    /// together — the whole runs past it in the store's order. A step per
    /// shortcode, whatever each holds, and exact whether or not a run is
    /// dense: `held` is a count, not a sequence.
    pub(super) fn held_after(&self, epic: &ShortcodeId) -> u64 {
        use std::ops::Bound::{Excluded, Unbounded};
        self.runs
            .range::<ShortcodeId, _>((Excluded(epic), Unbounded))
            .map(|(_, run)| run.held)
            .sum()
    }

    /// SCALE-19: how many tickets of `key`'s shortcode follow `key` (a
    /// ticket in it) — `last − seq`, when the run is dense and so that is
    /// the count. `None` when it is not: the sequence no longer says, and
    /// the caller counts.
    pub(super) fn after_in_run(&self, key: &OrderKey) -> Option<u64> {
        let run = self.runs.get(&key.shortcode).filter(|run| run.dense())?;
        Some(u64::from(run.last.saturating_sub(key.seq)))
    }

    /// How many ids are named before they exist — for the startup log.
    pub(super) fn unborn_count(&self) -> usize {
        self.unborn.len()
    }

    /// A ticket is born (`rows` hold it): its shortcode's sequence reaches
    /// it, the forward references to its id are resolved — they were read
    /// into its `unlocks` before the commit — and the ones it makes itself
    /// are kept. SCALE-19: and its shortcode holds one more. Born means
    /// new: `create` allocates past the highest sequence, so the newborn is
    /// never a ticket the run already counted.
    ///
    /// SCALE-28: worked out here, before the write commits, and moved by
    /// [`Self::born`] after — so what the write's transaction stores
    /// ([`Birth::store`]) and what the cache becomes are one value, not two
    /// computations that could disagree.
    pub(super) fn birth(&self, rows: Rows<'_>, t: &Row) -> Birth {
        let run = match self.runs.get(&t.shortcode) {
            Some(run) => Run {
                first: run.first.min(t.seq),
                last: run.last.max(t.seq),
                held: run.held + 1,
            },
            None => Run {
                first: t.seq,
                last: t.seq,
                held: 1,
            },
        };
        // A ticket naming one id twice is one entry, as a scan finds it once.
        let mut named: Vec<TicketName> = Vec::new();
        for b in &t.blocked_by {
            if !rows.contains(b) && !named.contains(b) {
                named.push(b.clone());
            }
        }
        Birth {
            epic: t.shortcode.clone(),
            run,
            id: t.id.clone(),
            resolves: self.unborn.contains_key(t.id.as_str()),
            names: named
                .into_iter()
                .map(|b| ((b, OrderKey::of(t)), t.id.clone()))
                .collect(),
        }
    }

    /// …and once it has committed, the cache takes it.
    pub(super) fn born(&mut self, birth: Birth) {
        self.runs.insert(birth.epic, birth.run);
        self.unborn.remove(birth.id.as_str());
        for ((id, key), referrer) in birth.names {
            self.unborn.entry(id).or_default().insert(key, referrer);
        }
    }

    /// MENGER-30: `t`'s `blocked_by` moved after birth (`update`) — of
    /// `added` and `removed`, each that names no ticket is kept as a forward
    /// reference from now on, or no longer. Worked out before the write
    /// commits, moved by [`Self::reblocked`] after.
    pub(super) fn reblock(&self, rows: Rows<'_>, t: &Row, added: &[TicketName], removed: &[TicketName]) -> Reblock {
        let key = OrderKey::of(t);
        let dangling = |ids: &[TicketName]| -> Vec<RefKey> {
            ids.iter().filter(|b| !rows.contains(b)).map(|b| (b.clone(), key.clone())).collect()
        };
        Reblock {
            id: t.id.clone(),
            named: dangling(added),
            unnamed: dangling(removed),
        }
    }

    /// …and once it has committed, the cache takes it.
    pub(super) fn reblocked(&mut self, r: Reblock) {
        for (id, key) in r.unnamed {
            if let Some(waiting) = self.unborn.get_mut(&id) {
                waiting.remove(&key);
                if waiting.is_empty() {
                    self.unborn.remove(&id);
                }
            }
        }
        for (id, key) in r.named {
            self.unborn.entry(id).or_default().insert(key, r.id.clone());
        }
    }

    /// SCALE-28: from what is stored — at start, instead of [`Self::build`].
    pub(super) fn from_stored(runs: BTreeMap<ShortcodeId, RunRow>, refs: BTreeMap<RefKey, TicketId>) -> Self {
        let mut unborn: HashMap<TicketName, BTreeMap<OrderKey, TicketId>> = HashMap::new();
        for ((id, key), referrer) in refs {
            unborn.entry(id).or_default().insert(key, referrer);
        }
        Self {
            runs: runs
                .into_iter()
                .map(|(epic, (first, last, held))| (epic, Run { first, last, held }))
                .collect(),
            unborn,
        }
    }

    /// SCALE-28: as stored — each shortcode's run, and each dangling forward
    /// reference keyed by the id it names and the naming ticket's position.
    pub(super) fn stored(&self) -> (BTreeMap<ShortcodeId, RunRow>, BTreeMap<RefKey, TicketId>) {
        let runs = self
            .runs
            .iter()
            .map(|(epic, run)| (epic.clone(), (run.first, run.last, run.held)))
            .collect();
        let refs = self
            .unborn
            .iter()
            .flat_map(|(id, waiting)| {
                waiting
                    .iter()
                    .map(move |(key, referrer)| ((id.clone(), key.clone()), referrer.clone()))
            })
            .collect();
        (runs, refs)
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
            .map(|(id, waiting)| (id.to_string(), waiting.values().map(TicketId::to_string).collect()))
            .collect()
    }

    /// Every shortcode's run as `(lowest sequence, highest, how many it
    /// holds, dense)` — for the tests' comparison with a scan (SCALE-19).
    #[cfg(test)]
    pub(super) fn runs(&self) -> BTreeMap<ShortcodeId, (u32, u32, u64, bool)> {
        self.runs
            .iter()
            .map(|(epic, run)| (epic.clone(), (run.first, run.last, run.held, run.dense())))
            .collect()
    }
}

/// SCALE-28: what one birth does to the indexes ([`Ids::birth`]) — the
/// newborn's shortcode's run as it will stand, whether references to its id
/// leave, and the ones it makes.
pub(super) struct Birth {
    epic: ShortcodeId,
    run: Run,
    id: TicketId,
    /// Some ticket named the newborn's id before it existed.
    resolves: bool,
    names: Vec<(RefKey, TicketId)>,
}

impl Birth {
    /// The rows the birth's own transaction writes: one run, and a
    /// reference only where one leaves or is made — almost never.
    pub(super) fn store(&self, into: &mut Changes) {
        into.runs
            .push((self.epic.clone(), (self.run.first, self.run.last, self.run.held)));
        if self.resolves {
            into.refs_born.push(self.id.clone());
        }
        into.refs_named.extend(self.names.iter().cloned());
    }
}

/// MENGER-30: what one `update` of a ticket's `blocked_by` does to the
/// forward references ([`Ids::reblock`]) — the ones it makes and the ones it
/// drops, each keyed by the id it names and the ticket's position.
pub(super) struct Reblock {
    id: TicketId,
    named: Vec<RefKey>,
    unnamed: Vec<RefKey>,
}

impl Reblock {
    /// The rows its transaction writes: a reference made or dropped — almost
    /// never.
    pub(super) fn store(&self, into: &mut Changes) {
        into.refs_named.extend(self.named.iter().map(|k| (k.clone(), self.id.clone())));
        into.refs_unnamed.extend(self.unnamed.iter().cloned());
    }
}
