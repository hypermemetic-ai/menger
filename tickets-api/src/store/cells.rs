//! FILTER-4: the cells — what a filter asks of a ticket without reading it.
//!
//! Every ticket has exactly one cell: its status, its kind, its severity,
//! whether it is `blocked` (its kept count of unsatisfied blockers is above
//! 0, SCALE-32), whether it `has commits` (SDLC-9) and — FILTER-21 —
//! whether it is a `menger doc` (its path starts with `_MENGER/`, WAVE-10;
//! set at birth, so only a birth moves it). A cell is interned
//! into the cell table on first use and named by its id from then on. Per
//! shortcode and per cell it holds, the sequences of the shortcode's tickets
//! in that cell are kept as a Roaring bitmap; its count is the bitmap's
//! length. A filter is compiled against the cell table — for each shortcode,
//! the cells it admits — and every answer is a sum or a merge over the
//! admitted bitmaps (FILTER-3 §6.1): nothing per ticket is kept, and no
//! ticket is tested.
//!
//! The SCALE-32 shape (`ready.rs`): what one write does to the cells is
//! worked out before it commits ([`Cells::moved`]), stored by the write's
//! own transaction ([`Move::store`]: the cells it interned, each container
//! it rewrote, each count it moved) and taken by the cache after the commit
//! ([`Cells::take`]) — one value in two places. A start reads the cells from
//! the file ([`Cells::from_stored`]); a build counts them by a scan
//! ([`Cells::build`]); `--verify-derived` compares the two
//! ([`super::stored::cell_differences`]).
//!
//! Stored form 4: `cells (id, state, kind, severity, blocked, commits)` —
//! form 6 (FILTER-21) adds `menger` —
//! `cell_bits (epic, cell, hi, card, bits)` — one row per 2¹⁶-sequence
//! container, `bits` the container serialized as Roaring serializes one: at
//! most 4,096 members as their low 16 bits, little-endian, two bytes each;
//! more as the 8 KiB bitmap of the block — and `cell_counts (epic, cell,
//! held)`.

use super::stored::Changes;
use super::{Kind, OrderKey, Row, Rows, Severity, Status};
use roaring::RoaringBitmap;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

/// A severity as a cell holds it: none, then the four, in the language's
/// order (`critical`, `high`, `medium`, `low`).
pub(super) const SEVERITIES: [Option<Severity>; 5] = [
    None,
    Some(Severity::Critical),
    Some(Severity::High),
    Some(Severity::Medium),
    Some(Severity::Low),
];

/// The members a container holds as an array; one more is a bitmap
/// (Roaring's own threshold).
const ARRAY_MAX: usize = 4096;

/// One cell: `(status, kind, severity, blocked, has commits, menger doc)`,
/// each as its place in [`Status::ALL`], [`Kind::ALL`] and [`SEVERITIES`] —
/// so it is small, `Copy`, hashable and ordered without the wire enums
/// being any of those.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Key {
    status: u8,
    kind: u8,
    severity: u8,
    blocked: bool,
    commits: bool,
    /// FILTER-21: one of menger's own documents.
    menger: bool,
}

fn place<T: PartialEq>(all: &[T], v: &T) -> u8 {
    all.iter().position(|x| x == v).expect("every value is in its table") as u8
}

impl Key {
    pub(super) fn new(status: Status, kind: Kind, severity: Option<Severity>, blocked: bool, commits: bool, menger: bool) -> Self {
        Self {
            status: place(&Status::ALL, &status),
            kind: place(&Kind::ALL, &kind),
            severity: place(&SEVERITIES, &severity),
            blocked,
            commits,
            menger,
        }
    }

    /// A ticket's cell: its row as the write leaves it, and its count of
    /// unsatisfied blockers as the write leaves it. FILTER-21: its path says
    /// whether it is a menger doc ([`crate::is_menger_doc`]).
    pub(super) fn of(t: &Row, unsatisfied: u32) -> Self {
        Self::new(
            t.status,
            t.kind,
            t.severity,
            unsatisfied > 0,
            !t.commits.is_empty(),
            crate::is_menger_doc(t.path.as_deref()),
        )
    }

    pub fn status(self) -> Status {
        Status::ALL[self.status as usize]
    }

    pub fn kind(self) -> Kind {
        Kind::ALL[self.kind as usize]
    }

    pub fn severity(self) -> Option<Severity> {
        SEVERITIES[self.severity as usize]
    }

    pub fn blocked(self) -> bool {
        self.blocked
    }

    pub fn commits(self) -> bool {
        self.commits
    }

    /// FILTER-21: whether it is one of menger's own documents.
    pub fn menger(self) -> bool {
        self.menger
    }
}

/// As `--verify-derived` names a cell: `(Ready, build, none, blocked, no
/// commits)` — FILTER-21: `, menger doc` last when it is one.
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}, {}, {}, {}{})",
            self.status(),
            self.kind(),
            self.severity().map_or("none".to_string(), |s| s.as_str().to_ascii_lowercase()),
            if self.blocked { "blocked" } else { "not blocked" },
            if self.commits { "has commits" } else { "no commits" },
            if self.menger { ", menger doc" } else { "" },
        )
    }
}

/// One shortcode's cells: each it holds, by id, ascending, with the
/// sequences of its tickets in it. A cell nobody in the shortcode holds has
/// no entry — an empty bitmap is dropped when its last member leaves.
pub(super) type Held = Vec<(u32, RoaringBitmap)>;

/// The cell table and every shortcode's bitmaps.
#[derive(Clone, Default)]
pub(super) struct Cells {
    /// Id → cell, in the order they were interned.
    keys: Vec<Key>,
    /// Cell → id.
    ids: HashMap<Key, u32>,
    /// Per shortcode, in the store's (shortcode) order.
    held: BTreeMap<String, Held>,
}

/// One ticket whose cell a write moved: its position — its shortcode
/// shared by every move in it, so a fan-out of 10⁵ allocates nothing a
/// ticket — the cell it left (`None`: born) and the cell it joined. Its id
/// is its row's, looked up by whoever says it ([`super::Watcher`]: at most
/// [`RECOUNT_OVER`] a write).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellMove {
    pub epic: Arc<str>,
    pub seq: u32,
    pub from: Option<u32>,
    pub to: u32,
}

/// What one write's cell moves leave behind, as `watch { filter }` needs
/// it: the moves, in the store's order, and — for a write that moves more
/// than [`RECOUNT_OVER`] tickets — every count of each shortcode it moved
/// as the write leaves them, so a subscriber's `recount` is this write's
/// number and not a later one's.
#[derive(Debug, Clone, Default)]
pub struct Moves {
    pub moves: Vec<CellMove>,
    /// Shortcode → each cell it holds after the write, with its count.
    pub counts: Option<BTreeMap<String, Vec<(u32, u64)>>>,
}

/// More membership moves than this for one subscriber in one write, and it
/// is sent `recount`s instead (FILTER-4: the broadcast's capacity).
pub const RECOUNT_OVER: usize = 256;

/// SCALE-32's shape: what one write does to the cells, worked out before it
/// commits ([`Cells::moved`]), stored by its transaction ([`Move::store`])
/// and taken by the cache after ([`Cells::take`]).
#[derive(Default)]
pub(super) struct Move {
    /// Cells first used by this write, with the ids they take.
    interned: Vec<(u32, Key)>,
    /// Per (shortcode, cell) the write touches: who leaves it and who joins
    /// it — what the cache applies, as two set operations a bitmap.
    groups: Vec<(String, u32, RoaringBitmap, RoaringBitmap)>,
    /// Each ticket whose cell changes, in the store's order — and, past
    /// [`RECOUNT_OVER`], the counts after: what the write's broadcast
    /// carries.
    shared: Arc<Moves>,
    /// Each container the moves rewrite, as it is after them: `None` when
    /// it is left empty (its row goes).
    bits: Vec<((String, u32, u32), Option<(u64, Vec<u8>)>)>,
    /// Each (shortcode, cell) count the moves change, with its new value.
    counts: Vec<((String, u32), u64)>,
}

impl Move {
    /// The rows the write's own transaction writes for it.
    pub(super) fn store(&self, into: &mut Changes) {
        into.cells.extend(self.interned.iter().copied());
        into.cell_bits.extend(self.bits.iter().cloned());
        into.cell_counts.extend(self.counts.iter().cloned());
    }

    /// FILTER-13: `key`'s id as the write leaves the table — the cache's,
    /// or one this write interns.
    pub(super) fn id_of(&self, cells: &Cells, key: Key) -> u32 {
        cells
            .ids
            .get(&key)
            .copied()
            .or_else(|| self.interned.iter().find(|(_, k)| *k == key).map(|(id, _)| *id))
            .expect("a cell the write moves is interned")
    }

    /// What the write's broadcast says of it — `None` when it moves no
    /// ticket's cell.
    pub(super) fn announced(&self) -> Option<Arc<Moves>> {
        (!self.shared.moves.is_empty()).then(|| Arc::clone(&self.shared))
    }
}

/// The stored form of the cells: the table, each container and each count.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Stored {
    pub(super) keys: BTreeMap<u32, Key>,
    pub(super) bits: BTreeMap<(String, u32, u32), (u64, Vec<u8>)>,
    pub(super) counts: BTreeMap<(String, u32), u64>,
}

/// A container's members (their low 16 bits, ascending) as `cell_bits`
/// stores them — written out, for the tests to hold Roaring's own
/// serialization ([`containers`]) to.
#[cfg(test)]
pub(super) fn encode(lows: &[u16]) -> Vec<u8> {
    if lows.len() <= ARRAY_MAX {
        let mut out = Vec::with_capacity(lows.len() * 2);
        for v in lows {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    } else {
        let mut words = [0u64; 1024];
        for v in lows {
            words[(*v >> 6) as usize] |= 1u64 << (*v & 63);
        }
        let mut out = Vec::with_capacity(8192);
        for w in words {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }
}

/// The members of a stored container, or why it cannot be one: an array
/// must be `card` ascending values, a bitmap 8 KiB with `card` bits set.
pub(super) fn decode(card: u64, bits: &[u8]) -> Result<Vec<u16>, String> {
    let card = card as usize;
    if card == 0 {
        return Err("an empty container is stored as no row".into());
    }
    if card <= ARRAY_MAX {
        if bits.len() != card * 2 {
            return Err(format!("an array of {card} holds {} bytes, not {}", bits.len(), card * 2));
        }
        let lows: Vec<u16> = bits.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        if lows.windows(2).any(|w| w[0] >= w[1]) {
            return Err("an array's members are not ascending".into());
        }
        Ok(lows)
    } else {
        if bits.len() != 8192 {
            return Err(format!("a bitmap of {card} holds {} bytes, not 8192", bits.len()));
        }
        let mut lows = Vec::with_capacity(card);
        for (i, w) in bits.chunks_exact(8).enumerate() {
            let mut w = u64::from_le_bytes(w.try_into().expect("8 bytes"));
            while w != 0 {
                let b = w.trailing_zeros();
                lows.push((i as u32 * 64 + b) as u16);
                w &= w - 1;
            }
        }
        if lows.len() != card {
            return Err(format!("a bitmap said to hold {card} holds {}", lows.len()));
        }
        Ok(lows)
    }
}

/// FILTER-4: a start's read of `cell_bits`, row by row in `(epic, cell,
/// hi)` order — each container checked ([`decode`]) and gathered with its
/// (shortcode, cell)'s others into Roaring's portable serialization, which
/// is what a stored container already is the body of; a bitmap is then
/// deserialized whole, its arrays allocated at their size. No row is held
/// past its bitmap: what a start keeps of the cells is the bitmaps.
#[derive(Default)]
pub(super) struct Filling {
    held: BTreeMap<String, Held>,
    epic: String,
    here: Held,
    cell: Option<u32>,
    /// The current bitmap's containers: `(hi, card)` and their bodies.
    heads: Vec<(u16, u16)>,
    body: Vec<u8>,
}

/// Roaring's cookie for a serialization without run containers.
const SERIAL_COOKIE_NO_RUNCONTAINER: u32 = 12346;

impl Filling {
    /// One stored container, in `(epic, cell, hi)` order.
    pub(super) fn push(&mut self, epic: &str, cell: u32, hi: u32, card: u64, bits: &[u8]) -> Result<(), String> {
        let at = |why: String| format!("cell_bits {epic} cell {cell} container {hi}: {why}");
        decode(card, bits).map_err(at)?;
        if hi > u32::from(u16::MAX) {
            return Err(at("a block is at most 65535".into()));
        }
        if epic != self.epic {
            self.flush()?;
            let held = std::mem::take(&mut self.here);
            if !self.epic.is_empty() || !held.is_empty() {
                self.held.insert(std::mem::take(&mut self.epic), held);
            }
            self.epic = epic.to_string();
        } else if self.cell != Some(cell) {
            self.flush()?;
        }
        if self.heads.last().is_some_and(|(h, _)| u32::from(*h) >= hi) || self.here.last().is_some_and(|(c, _)| *c >= cell && self.cell != Some(cell)) {
            return Err(at("containers out of order".into()));
        }
        self.cell = Some(cell);
        self.heads.push((hi as u16, (card - 1) as u16));
        self.body.extend_from_slice(bits);
        Ok(())
    }

    /// The bitmap gathered so far, deserialized into its shortcode's cells.
    fn flush(&mut self) -> Result<(), String> {
        let Some(cell) = self.cell.take() else { return Ok(()) };
        let n = self.heads.len();
        let mut buf = Vec::with_capacity(8 + 8 * n + self.body.len());
        buf.extend_from_slice(&SERIAL_COOKIE_NO_RUNCONTAINER.to_le_bytes());
        buf.extend_from_slice(&(n as u32).to_le_bytes());
        for (hi, card) in &self.heads {
            buf.extend_from_slice(&hi.to_le_bytes());
            buf.extend_from_slice(&card.to_le_bytes());
        }
        // Offsets, which a reader into memory passes over.
        buf.resize(buf.len() + 4 * n, 0);
        buf.extend_from_slice(&self.body);
        let bm = RoaringBitmap::deserialize_from(&buf[..])
            .map_err(|e| format!("cell_bits {} cell {cell}: not a bitmap: {e}", self.epic))?;
        self.here.push((cell, bm));
        self.heads.clear();
        self.body.clear();
        Ok(())
    }

    /// Every shortcode's bitmaps.
    pub(super) fn finish(mut self) -> Result<BTreeMap<String, Held>, String> {
        self.flush()?;
        if !self.here.is_empty() {
            self.held.insert(self.epic, self.here);
        }
        for h in self.held.values_mut() {
            h.shrink_to_fit();
        }
        Ok(self.held)
    }
}

/// Every container of `bm`, as `cell_bits` stores it: `(hi, card, bits)` —
/// cut from Roaring's own serialization, whose container bodies are exactly
/// the stored form: Roaring keeps a container an array of its members' low
/// 16 bits up to 4,096 of them and the block's 8 KiB bitmap past it.
pub(super) fn containers(bm: &RoaringBitmap) -> Vec<(u32, u64, Vec<u8>)> {
    let mut buf = Vec::with_capacity(bm.serialized_size());
    bm.serialize_into(&mut buf).expect("a Vec takes every byte");
    let n = u32::from_le_bytes(buf[4..8].try_into().expect("4 bytes")) as usize;
    let mut at = 8 + 8 * n;
    (0..n)
        .map(|i| {
            let head = &buf[8 + 4 * i..8 + 4 * i + 4];
            let hi = u32::from(u16::from_le_bytes([head[0], head[1]]));
            let card = u64::from(u16::from_le_bytes([head[2], head[3]])) + 1;
            let len = if card as usize <= ARRAY_MAX { 2 * card as usize } else { 8192 };
            let bits = buf[at..at + len].to_vec();
            at += len;
            debug_assert_eq!(decode(card, &bits).map(|l| l.len() as u64), Ok(card));
            (hi, card, bits)
        })
        .collect()
}

fn find(held: &Held, cell: u32) -> Option<&RoaringBitmap> {
    held.binary_search_by_key(&cell, |(c, _)| *c).ok().map(|i| &held[i].1)
}

impl Cells {
    /// Every ticket's cell, from the rows and the counts as kept — at a
    /// build, after a seed, and for `--verify-derived` to compare with. The
    /// rows arrive in the store's order, so each bitmap is built by pushing.
    pub(super) fn build(rows: Rows<'_>, unsatisfied: impl Fn(&str) -> u32) -> Self {
        let mut c = Self::default();
        let mut here: Option<(String, Held)> = None;
        for t in rows.tickets() {
            let id = c.intern(Key::of(t, unsatisfied(&t.id)));
            if here.as_ref().is_none_or(|(e, _)| *e != t.epic) {
                if let Some((e, held)) = here.take() {
                    c.held.insert(e, held);
                }
                here = Some((t.epic.clone(), Vec::new()));
            }
            let held = &mut here.as_mut().expect("set above").1;
            let at = match held.binary_search_by_key(&id, |(c, _)| *c) {
                Ok(i) => i,
                Err(i) => {
                    held.insert(i, (id, RoaringBitmap::new()));
                    i
                }
            };
            let bm = &mut held[at].1;
            if !bm.push(t.seq) {
                bm.insert(t.seq);
            }
        }
        if let Some((e, held)) = here {
            c.held.insert(e, held);
        }
        c
    }

    fn intern(&mut self, key: Key) -> u32 {
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        let id = self.keys.len() as u32;
        self.keys.push(key);
        self.ids.insert(key, id);
        id
    }

    /// SCALE-28's read at start: the cell table, and each shortcode's
    /// bitmaps as [`Filling`] made them from the stored containers. The ids
    /// are the file's.
    pub(super) fn from_stored(keys: BTreeMap<u32, Key>, held: BTreeMap<String, Held>) -> Result<Self, String> {
        let mut c = Self::default();
        for (id, key) in keys {
            if id as usize != c.keys.len() {
                return Err(format!("cells: the ids are not 0, 1, 2, … (found {id} after {})", c.keys.len()));
            }
            if c.ids.insert(key, id).is_some() {
                return Err(format!("cells: {key} is interned twice"));
            }
            c.keys.push(key);
        }
        for (epic, h) in &held {
            if let Some((cell, _)) = h.iter().find(|(cell, _)| *cell as usize >= c.keys.len()) {
                return Err(format!("cell_bits {epic} cell {cell}: no such cell"));
            }
        }
        c.held = held;
        Ok(c)
    }

    /// As stored ([`Stored`]).
    pub(super) fn stored(&self) -> Stored {
        let mut s = Stored {
            keys: self.keys.iter().enumerate().map(|(i, k)| (i as u32, *k)).collect(),
            ..Stored::default()
        };
        for (epic, held) in &self.held {
            for (cell, bm) in held {
                s.counts.insert((epic.clone(), *cell), bm.len());
                for (hi, card, bits) in containers(bm) {
                    s.bits.insert((epic.clone(), *cell, hi), (card, bits));
                }
            }
        }
        s
    }

    /// The cell table: id → cell.
    pub(super) fn keys(&self) -> &[Key] {
        &self.keys
    }

    /// FILTER-13: `key`'s id, when it is interned.
    pub(super) fn id(&self, key: Key) -> Option<u32> {
        self.ids.get(&key).copied()
    }

    /// FILTER-13: `epic`'s cells and their bitmaps.
    pub(super) fn held_in(&self, epic: &str) -> Option<&Held> {
        self.held.get(epic)
    }

    /// Each shortcode that holds a ticket, in the store's order, with each
    /// cell it holds and its bitmap.
    pub(super) fn shortcodes(&self) -> impl Iterator<Item = (&str, &Held)> {
        self.held.iter().map(|(e, h)| (e.as_str(), h))
    }

    /// How many (shortcode, cell) bitmaps there are — for the logs.
    pub(super) fn bitmaps(&self) -> usize {
        self.held.values().map(Vec::len).sum()
    }

    /// How many tickets they hold, all told.
    pub(super) fn tickets(&self) -> u64 {
        self.held.values().flat_map(|h| h.iter().map(|(_, b)| b.len())).sum()
    }

    /// The bytes the bitmaps serialize to — for the logs.
    pub(super) fn serialized_size(&self) -> usize {
        self.held.values().flat_map(|h| h.iter().map(|(_, b)| b.serialized_size())).sum()
    }

    /// `key`'s id — as the cache holds it, or as `interned` (this write's
    /// own new cells) will.
    fn id_in(&self, key: Key, interned: &mut Vec<(u32, Key)>) -> u32 {
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        if let Some((id, _)) = interned.iter().find(|(_, k)| *k == key) {
            return *id;
        }
        let id = (self.keys.len() + interned.len()) as u32;
        interned.push((id, key));
        id
    }

    /// What a write does to the cells: each of `touched` — a ticket as the
    /// write leaves it, the cell it was in (`None`: born now) and the cell
    /// its row and count put it in — that changes cell moves; then each
    /// container and count the moves change, as the cache will hold them
    /// once it takes them. From the cache as it stands, under the
    /// write-serial lock: nothing else moves it in between.
    pub(super) fn moved<'a>(&self, touched: impl IntoIterator<Item = (&'a Row, Option<Key>, Key)>) -> Move {
        let mut m = Move::default();
        let mut moves: Vec<CellMove> = Vec::new();
        let mut epics: HashMap<&'a str, Arc<str>> = HashMap::new();
        // The cells a write moves between are few; the last seen are kept
        // beside the table's lookup.
        let mut seen: Vec<(Key, u32)> = Vec::new();
        for (t, before, after) in touched {
            if before == Some(after) {
                continue;
            }
            let mut id_of = |k: Key, interned: &mut Vec<(u32, Key)>| match seen.iter().find(|(x, _)| *x == k) {
                Some((_, id)) => *id,
                None => {
                    let id = self.id_in(k, interned);
                    seen.push((k, id));
                    id
                }
            };
            let from = before.map(|k| id_of(k, &mut m.interned));
            let to = id_of(after, &mut m.interned);
            let epic = epics.entry(t.epic.as_str()).or_insert_with(|| Arc::from(t.epic.as_str()));
            moves.push(CellMove {
                epic: Arc::clone(epic),
                seq: t.seq,
                from,
                to,
            });
        }
        if moves.is_empty() {
            return m;
        }
        // Mostly in order already (a blocker's `unlocks` is): a pass.
        moves.sort_by(|a, b| (&*a.epic, a.seq).cmp(&(&*b.epic, b.seq)));
        // Per (shortcode, cell): who leaves it and who joins it, ascending.
        let mut patch: BTreeMap<(&str, u32), (Vec<u32>, Vec<u32>)> = BTreeMap::new();
        for mv in &moves {
            if let Some(from) = mv.from {
                patch.entry((&*mv.epic, from)).or_default().0.push(mv.seq);
            }
            patch.entry((&*mv.epic, mv.to)).or_default().1.push(mv.seq);
        }
        let empty = RoaringBitmap::new();
        for ((epic, cell), (leave, join)) in patch {
            let bm = self.held.get(epic).and_then(|h| find(h, cell)).unwrap_or(&empty);
            let leave = RoaringBitmap::from_sorted_iter(leave).expect("moves are sorted");
            let join = RoaringBitmap::from_sorted_iter(join).expect("moves are sorted");
            // The blocks the moves touch, as they will be: what the cache
            // holds there, less who leaves, plus who joins — three set
            // operations, a container at a time; then each block a row.
            let mut mask = RoaringBitmap::new();
            let mut blocks: Vec<u32> = leave.iter().chain(join.iter()).map(|s| s >> 16).collect();
            blocks.sort_unstable();
            blocks.dedup();
            for hi in &blocks {
                mask.insert_range((hi << 16)..=((hi << 16) | 0xFFFF));
            }
            let mut after = bm & &mask;
            after -= &leave;
            after |= &join;
            let mut rows: BTreeMap<u32, (u64, Vec<u8>)> = containers(&after).into_iter().map(|(hi, card, bits)| (hi, (card, bits))).collect();
            for hi in blocks {
                m.bits.push(((epic.to_string(), cell, hi), rows.remove(&hi)));
            }
            let held = bm.len() - bm.intersection_len(&leave) + join.difference_len(bm);
            m.counts.push(((epic.to_string(), cell), held));
            m.groups.push((epic.to_string(), cell, leave, join));
        }
        let mut snapshot: Option<BTreeMap<String, Vec<(u32, u64)>>> = None;
        if moves.len() > RECOUNT_OVER {
            // Every count of each shortcode moved, as the write leaves it.
            let mut counts_after: BTreeMap<String, Vec<(u32, u64)>> = BTreeMap::new();
            let moved: BTreeMap<(&str, u32), u64> = m.counts.iter().map(|((e, c), n)| ((e.as_str(), *c), *n)).collect();
            for epic in moves.iter().map(|mv| &*mv.epic).collect::<BTreeSet<_>>() {
                let mut counts: BTreeMap<u32, u64> = self
                    .held
                    .get(epic)
                    .map(|h| h.iter().map(|(c, b)| (*c, b.len())).collect())
                    .unwrap_or_default();
                for ((_, c), n) in moved.range((epic, 0)..=(epic, u32::MAX)) {
                    counts.insert(*c, *n);
                }
                counts_after.insert(epic.to_string(), counts.into_iter().filter(|(_, n)| *n > 0).collect());
            }
            snapshot = Some(counts_after);
        }
        m.shared = Arc::new(Moves { moves, counts: snapshot });
        m
    }

    /// …and once it has committed, the cache takes it: the cells it
    /// interned, then each (shortcode, cell) it touched less who left and
    /// plus who joined.
    pub(super) fn take(&mut self, m: Move) {
        for (id, key) in m.interned {
            debug_assert_eq!(id as usize, self.keys.len(), "a write's cells continue the table");
            self.keys.push(key);
            self.ids.insert(key, id);
        }
        for (epic, cell, leave, join) in m.groups {
            let held = match self.held.get_mut(&epic) {
                Some(h) => h,
                None => self.held.entry(epic.clone()).or_default(),
            };
            let at = match held.binary_search_by_key(&cell, |(c, _)| *c) {
                Ok(i) => i,
                Err(i) => {
                    held.insert(i, (cell, RoaringBitmap::new()));
                    i
                }
            };
            let bm = &mut held[at].1;
            *bm -= &leave;
            *bm |= &join;
            if bm.is_empty() {
                held.remove(at);
                if held.is_empty() {
                    self.held.remove(&epic);
                }
            }
        }
    }

    /// The cell `seq` of `epic` is in, by looking — for the tests.
    #[cfg(test)]
    pub(super) fn cell_of(&self, key: &OrderKey) -> Option<Key> {
        self.held
            .get(&key.epic)?
            .iter()
            .find(|(_, b)| b.contains(key.seq))
            .map(|(c, _)| self.keys[*c as usize])
    }

    /// Every ticket's cell, by position — the scan's shape, for the tests.
    #[cfg(test)]
    pub(super) fn by_ticket(&self) -> BTreeMap<OrderKey, Key> {
        let mut out = BTreeMap::new();
        for (epic, held) in &self.held {
            for (cell, bm) in held {
                for seq in bm {
                    let prev = out.insert(OrderKey { epic: epic.clone(), seq }, self.keys[*cell as usize]);
                    assert!(prev.is_none(), "{epic}-{seq} is in two cells");
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Reads: a filter compiled against the cell table, and the answers that are
// sums and merges over the bitmaps it admits.

/// A filter compiled against the cell table (FILTER-3 §6.1): for each
/// shortcode, which cells it admits. Shortcodes the filter names (in an `in`)
/// each have their own table; every other shortcode shares one, since
/// nothing in the filter can tell them apart.
#[derive(Debug, Clone)]
pub(super) struct Admit {
    /// The cells admitted in a shortcode the filter does not name, by id.
    others: Vec<bool>,
    /// The cells admitted in each shortcode it names.
    named: BTreeMap<String, Vec<bool>>,
}

impl Admit {
    /// `eval(shortcode, cell)` once per cell of the table for each shortcode
    /// in `named` and once for the rest (`""`, which no `in` names). Never
    /// once per ticket.
    pub(super) fn compile(keys: &[Key], named: impl IntoIterator<Item = String>, eval: impl Fn(&str, Key) -> bool) -> Self {
        let table = |epic: &str| keys.iter().map(|k| eval(epic, *k)).collect::<Vec<bool>>();
        let named: BTreeMap<String, Vec<bool>> = named.into_iter().map(|e| {
            let t = table(&e);
            (e, t)
        }).collect();
        Self { others: table(""), named }
    }

    /// The cells `epic` admits, by id. A cell past the table (interned
    /// after the compile) is not admitted: compile again to see it. A
    /// shortcode is named whatever its case — `in` reads it as the language
    /// does, case-insensitively (only a hand edit writes a lower-case one).
    pub(super) fn of(&self, epic: &str) -> &[bool] {
        let named = match self.named.get(epic) {
            Some(t) => Some(t),
            None if epic.bytes().any(|b| b.is_ascii_lowercase()) => self.named.get(&epic.to_ascii_uppercase()),
            None => None,
        };
        named.map_or(&self.others, Vec::as_slice)
    }

    /// How many cells were compiled — one past the highest id it knows.
    pub(super) fn len(&self) -> usize {
        self.others.len()
    }

    /// Whether `cell` of `epic` is admitted; `None` past the table.
    pub(super) fn admits(&self, epic: &str, cell: u32) -> Option<bool> {
        self.of(epic).get(cell as usize).copied()
    }
}

/// The matches of one query, per shortcode in the store's order: the
/// bitmaps whose union they are. Cell bitmaps are disjoint (a ticket is in
/// one cell), so a count is a sum of lengths and a rank a sum of ranks; a
/// restriction (a parent's children) is one bitmap of its own.
pub(super) struct View<'a> {
    by_epic: Vec<(&'a str, Vec<std::borrow::Cow<'a, RoaringBitmap>>)>,
}

impl<'a> View<'a> {
    /// FILTER-13: a view made elsewhere — per shortcode, in the store's
    /// order, the disjoint bitmaps whose union is its matches.
    pub(super) fn from_parts(by_epic: Vec<(&'a str, Vec<std::borrow::Cow<'a, RoaringBitmap>>)>) -> Self {
        Self { by_epic }
    }

    /// The same, inside the sequences `within` gives each shortcode (a
    /// parent's children): one bitmap a shortcode, their intersection.
    pub(super) fn within(self, within: impl Fn(&str) -> Option<RoaringBitmap>) -> View<'a> {
        let by_epic = self
            .by_epic
            .into_iter()
            .filter_map(|(epic, bms)| {
                let keep = within(epic)?;
                let mut union = RoaringBitmap::new();
                for b in &bms {
                    union |= b.as_ref();
                }
                union &= keep;
                (!union.is_empty()).then(|| (epic, vec![std::borrow::Cow::Owned(union)]))
            })
            .collect();
        View { by_epic }
    }

    /// Each shortcode with a match, and how many.
    pub(super) fn counts(&self) -> impl Iterator<Item = (&'a str, u64)> + '_ {
        self.by_epic
            .iter()
            .map(|(e, bms)| (*e, bms.iter().map(|b| b.len()).sum::<u64>()))
            .filter(|(_, n)| *n > 0)
    }

    /// Every match.
    pub(super) fn total(&self) -> u64 {
        self.counts().map(|(_, n)| n).sum()
    }

    /// How many matches come before the first position past `key` — the
    /// matches at or before it, in the store's order. `key` need not match.
    pub(super) fn rank(&self, key: &OrderKey) -> u64 {
        let mut n = 0;
        for (epic, bms) in &self.by_epic {
            match (*epic).cmp(key.epic.as_str()) {
                std::cmp::Ordering::Less => n += bms.iter().map(|b| b.len()).sum::<u64>(),
                std::cmp::Ordering::Equal => n += bms.iter().map(|b| b.rank(key.seq)).sum::<u64>(),
                std::cmp::Ordering::Greater => break,
            }
        }
        n
    }

    /// The `p`-th match (0-based), or `None` past the last.
    pub(super) fn select(&self, p: u64) -> Option<(&'a str, u32)> {
        let mut left = p;
        for (epic, bms) in &self.by_epic {
            let n: u64 = bms.iter().map(|b| b.len()).sum();
            if left >= n {
                left -= n;
                continue;
            }
            return Some((*epic, select_in(bms, left)));
        }
        None
    }

    /// At most `limit` matches from `from` on (inclusive), in the store's
    /// order — a merge of the admitted bitmaps, a shortcode at a time.
    pub(super) fn page(&self, from: Option<(&str, u32)>, limit: usize) -> Vec<(&'a str, u32)> {
        let start = match from {
            None => 0,
            Some((epic, _)) => self.by_epic.partition_point(|(e, _)| *e < epic),
        };
        self.page_from(start, from, limit)
    }

    /// At most `limit` matches strictly after `key`, which need not match.
    pub(super) fn page_after(&self, key: &OrderKey, limit: usize) -> Vec<(&'a str, u32)> {
        match key.seq.checked_add(1) {
            Some(seq) => self.page(Some((key.epic.as_str(), seq)), limit),
            None => {
                let start = self.by_epic.partition_point(|(e, _)| *e <= key.epic.as_str());
                self.page_from(start, None, limit)
            }
        }
    }

    fn page_from(&self, start: usize, from: Option<(&str, u32)>, limit: usize) -> Vec<(&'a str, u32)> {
        let mut out = Vec::with_capacity(limit.min(1024));
        if limit == 0 {
            return out;
        }
        for (epic, bms) in &self.by_epic[start..] {
            let lo = match from {
                Some((e, seq)) if e == *epic => seq,
                _ => 0,
            };
            merge(bms, lo, limit - out.len(), |seq| out.push((*epic, seq)));
            if out.len() >= limit {
                break;
            }
        }
        out
    }
}

/// The `k`-th member (0-based) of the union of the disjoint `bms` — a
/// binary search over the sequence, rank summed over the bitmaps; one
/// bitmap selects directly.
fn select_in(bms: &[std::borrow::Cow<'_, RoaringBitmap>], k: u64) -> u32 {
    if let [one] = bms {
        return one.select(k as u32).expect("k is below the count");
    }
    let (mut lo, mut hi) = (
        bms.iter().filter_map(|b| b.min()).min().expect("a match"),
        bms.iter().filter_map(|b| b.max()).max().expect("a match"),
    );
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let r: u64 = bms.iter().map(|b| b.rank(mid)).sum();
        if r > k {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

/// The union of the disjoint `bms`, from `lo` on, in order: at most `take`
/// members, each handed to `out` — a k-way merge.
fn merge(bms: &[std::borrow::Cow<'_, RoaringBitmap>], lo: u32, take: usize, mut out: impl FnMut(u32)) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    if let [one] = bms {
        one.range(lo..).take(take).for_each(out);
        return;
    }
    let mut its: Vec<_> = bms.iter().map(|b| b.range(lo..)).collect();
    let mut heap = BinaryHeap::with_capacity(its.len());
    for (i, it) in its.iter_mut().enumerate() {
        if let Some(v) = it.next() {
            heap.push(Reverse((v, i)));
        }
    }
    let mut n = 0;
    while let Some(Reverse((v, i))) = heap.pop() {
        out(v);
        n += 1;
        if n == take {
            return;
        }
        if let Some(next) = its[i].next() {
            heap.push(Reverse((next, i)));
        }
    }
}

// ---------------------------------------------------------------------------
// FILTER-15: the reads that take the lens ask a view two more things — is
// this ticket a match (one lookup in its shortcode's admitted bitmaps), and
// which bitmaps hold the candidates from a shortcode on.

impl<'a> View<'a> {
    /// Whether the ticket at `seq` of `epic` is a match: its shortcode found
    /// by a binary search, then a lookup in each bitmap admitted there — the
    /// cells are disjoint, so the first that holds it answers.
    pub(super) fn contains(&self, epic: &str, seq: u32) -> bool {
        match self.by_epic.binary_search_by(|(e, _)| (*e).cmp(epic)) {
            Ok(i) => self.by_epic[i].1.iter().any(|b| b.contains(seq)),
            Err(_) => false,
        }
    }

    /// Each shortcode with a match from `epic` on (every one without it), in
    /// the store's order, with the bitmaps admitted there.
    pub(super) fn shortcodes_from(&self, epic: Option<&str>) -> &[(&'a str, Vec<std::borrow::Cow<'a, RoaringBitmap>>)] {
        let start = epic.map_or(0, |e| self.by_epic.partition_point(|(x, _)| *x < e));
        &self.by_epic[start..]
    }
}
