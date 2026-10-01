use super::{OrderKey, Rows, Ticket};
use std::collections::{BTreeMap, HashMap};

pub(super) struct Ids {
    /// Each shortcode's highest sequence, in shortcode order — the
    /// store's order, byte-lexicographic on the epic. Nothing deletes a
    /// ticket, so this only ever rises, and plus one is the next id.
    last: BTreeMap<String, u32>,
    /// The dangling forward references, kept from the end they name: an
    /// id that some ticket's `blocked_by` names and that is no ticket yet
    /// → those tickets, in the store's order. An entry leaves when its id
    /// is born; one that can never be (`RT-0`, a malformed id) stays, as
    /// it would in a scan.
    unborn: HashMap<String, BTreeMap<OrderKey, String>>,
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
    pub(super) fn build(rows: Rows<'_>) -> Self {
        use std::ops::Bound::{Excluded, Included, Unbounded};
        let mut ids = Self {
            last: BTreeMap::new(),
            unborn: HashMap::new(),
        };
        let mut next = rows.order.keys().next();
        while let Some(first) = next {
            let end = OrderKey {
                epic: first.epic.clone(),
                seq: u32::MAX,
            };
            let (last, _) = rows
                .order
                .range::<OrderKey, _>((Included(first), Included(&end)))
                .next_back()
                .expect("the run holds its own first key");
            ids.last.insert(end.epic.clone(), last.seq);
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
        self.last.get(epic).map_or(1, |seq| seq + 1)
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
        self.last.iter().map(|(epic, seq)| (epic.as_str(), *seq))
    }

    /// How many ids are named before they exist — for the startup log.
    pub(super) fn unborn_count(&self) -> usize {
        self.unborn.len()
    }

    /// A ticket is born (already in `rows`): its shortcode's sequence
    /// reaches it, the forward references to its id are resolved — they
    /// were read into its `unlocks` before the commit — and the ones it
    /// makes itself are kept.
    pub(super) fn born(&mut self, rows: Rows<'_>, t: &Ticket) {
        match self.last.get_mut(&t.epic) {
            Some(seq) => *seq = (*seq).max(t.seq),
            None => {
                self.last.insert(t.epic.clone(), t.seq);
            }
        }
        self.unborn.remove(&t.id);
        self.names(rows, t);
    }

    /// Keeps each id `t` names in `blocked_by` that is no ticket. A ticket
    /// naming one id twice is one entry, as a scan finds it once.
    fn names(&mut self, rows: Rows<'_>, t: &Ticket) {
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
}
