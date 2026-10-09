use super::cells::{self, Cells, Key};
use super::tree;
use super::{children::Children, db, ids::Ids, ready::Ready, rollup::Rollups, Cache, OrderKey, Row};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// Which form of the stored derived state this build writes and reads.
/// One table more (B3's edges, say) is a new form: a file holding an older
/// one is built again, once, as a file from before SCALE-28 is.
///
/// SCALE-32 (B3) is form 2: each ticket's count of unsatisfied blockers (a
/// column), the ready set (the file's index over it), each shortcode's
/// count of each status and its share of the ready set.
///
/// SDLC-11 is form 3: the roll-ups count docs apart from the work — a
/// `docs` beside each `(total, done)` of a group and of a cross tree — so a
/// file in form 2 is built again, once, on its first open (it holds no doc,
/// so every `docs` is 0 and every other number is what it was).
///
/// FILTER-4 is form 4: the cells — the cell table (`cells`), each
/// shortcode's bitmap of the sequences in each cell it holds, a row per
/// 2¹⁶-sequence container (`cell_bits`), and each one's count
/// (`cell_counts`). A file in form 3 or older is built again, once, on its
/// first open, and holds them from then on.
///
/// FILTER-13 is form 5: the kept tree — each internal ticket's tally of its
/// descendants by (shortcode, cell) (`tree_tallies`), and the holders of
/// each list of more than 256 children (`tree_holders`, a row per
/// container). A file in form 4 or older is built again, once, on its first
/// open.
///
/// FILTER-21 is form 6: a cell says whether its tickets are menger's own
/// documents (`menger doc`) — the cell table gains a `menger` column, and a
/// board note no longer shares a cell with an ordinary doc, so every cell id
/// the derived state names may move. A file in form 5 or older gains the
/// column and is built again, once, on its first open. One way: an older
/// build reads the table without the column, so a board note's cell reads
/// as an ordinary doc's — a file holding both is refused as interning one
/// cell twice (`promote`'s rollback restores the backup it took first).
const FORM: i64 = 6;

/// SDLC-11: the columns form 3 adds to the roll-up tables, `(table,
/// column)` — what a build gives a file from an older form, as it gives one
/// `child_count` and `unsatisfied`.
const DOCS_COLUMNS: [(&str, &str); 4] = [
    ("rollup_groups", "base_docs"),
    ("rollup_groups", "own_docs"),
    ("rollup_groups", "docs"),
    ("rollup_cross", "docs"),
];

/// FILTER-21: the column form 6 adds to the cell table, `(table, column)` —
/// what a build gives a file in form 4 or 5, as it gives the roll-ups their
/// `docs`.
const MENGER_COLUMN: (&str, &str) = ("cells", "menger");

/// The tables the derived state is stored in, and the index children are
/// read from — each keyed the way the read that will answer from it asks
/// (B4: nothing loaded at start), so a question is a point read or a range,
/// never a scan:
///
/// - `shortcode_runs` — a shortcode's run by its shortcode: the next id is
///   `last + 1`, `extents` is the table in key order, `page.remaining` and
///   `health.ticket_count` sum `held` (SCALE-3, SCALE-19).
/// - `forward_refs` — a dangling forward reference by the id it names, its
///   referrers in the store's order: what a newborn with that id unlocks
///   from birth (SCALE-3). `referrer` is the naming ticket's id, beside its
///   position.
/// - `rollup_groups` — MNG-10's roll-up of a shortcode, by its shortcode:
///   `groups` is `total`, `done`, `root` and `host` of every row with an
///   `own_total`, in key order. The rest is the state the roll-ups are kept
///   with (the pool of trees with no parent, what the row draws before any
///   nested group, the group it is drawn inside). SDLC-11: each tally's
///   docs beside it (`base_docs`, `own_docs`, `docs`) — a doc is in no
///   `total` and no `done` — and a row with `own_docs` alone is drawn.
/// - `rollup_cross` — the tally of each tree whose top hangs in another
///   shortcode (every sub-shortcode's root), and the group it is drawn in.
/// - `rollup_marks` — what only a hand edit makes: tickets on a parent cycle
///   and ids named as a parent that are no ticket (MNG-17).
/// - `tickets_by_parent` ([`CHILDREN_INDEX`]) — a ticket's children in the
///   store's order: a page of `list { parent }` is a range of it (SCALE-4);
///   and each ticket's own `child_count` is a column on its row (added
///   beside it).
/// - `tickets_ready` ([`READY_INDEX`]) — SCALE-32: the ready set, in the
///   store's order: an index over the tickets that are `Ready` with a count
///   of 0 in their `unsatisfied` column (added beside them: how many of the
///   ids a ticket names in `blocked_by` are not a `Complete` or `Superseded`
///   ticket). SQLite keeps it with the row, in the transaction that moves a
///   status or a count; a page of `ready_set` is a range of it.
/// - `status_counts` — SCALE-32: how many tickets of a shortcode hold a
///   status, by shortcode then status, a row for each that is not 0: what
///   `remaining` on a page of `list { status }` sums whole shortcodes from.
/// - `ready_counts` — SCALE-32: each shortcode's share of the ready set, a
///   row for each that holds any: what `remaining` on a page of `ready_set`
///   sums whole shortcodes from, so only the one a cursor cuts is counted
///   in the index.
/// - `cells` — FILTER-4: the cell table: each cell a ticket has been in,
///   `(state, kind, severity, blocked, commits, menger)`, by the id it was
///   interned with (`severity` NULL for none; `blocked`, `commits` and —
///   FILTER-21, form 6 — `menger` 0 or 1).
/// - `cell_bits` — FILTER-4: per shortcode and cell, the sequences of the
///   shortcode's tickets in that cell, a row per container of 2¹⁶
///   sequences (`hi`, the block): `card` members, `bits` the container
///   serialized as Roaring serializes one — at most 4,096 members as their
///   low 16 bits, little-endian; more as the block's 8 KiB bitmap. What a
///   filtered read is a merge of; at most 8 KiB a row whatever the store's
///   size, and a cell move rewrites two.
/// - `cell_counts` — FILTER-4: how many tickets each `(epic, cell)` holds,
///   a row for each that holds any: what `counts { filter }` sums.
/// - `tree_tallies` — FILTER-13: per internal ticket (`parent`, a ticket's
///   id or an id a hand edit names as a parent), how many of its
///   descendants are in each (`epic`, `cell`), a row for each that is not 0:
///   what `tree`'s `below` sums, and what a subtree's move takes off one
///   chain of ancestors and puts on another.
/// - `tree_holders` — FILTER-13: per parent and shortcode whose list of
///   children is longer than 256 (`kids`), and per (`epic`, `cell`) some
///   child's tally holds, the children whose tally holds it — a row per
///   2¹⁶-sequence container, as `cell_bits` stores one: what a parent's
///   children under a filter are a union of.
/// - `derived_state` — one row: which form is stored, and whether it is
///   behind the tickets (a seed commits its tickets a chunk at a time and
///   their derived state after the last; until then the row says so).
const SCHEMA: &str = "
        CREATE TABLE IF NOT EXISTS shortcode_runs (
            epic  TEXT PRIMARY KEY,
            first INTEGER NOT NULL,
            last  INTEGER NOT NULL,
            held  INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS forward_refs (
            id       TEXT NOT NULL,
            epic     TEXT NOT NULL,
            seq      INTEGER NOT NULL,
            referrer TEXT NOT NULL,
            PRIMARY KEY (id, epic, seq)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS rollup_groups (
            epic       TEXT PRIMARY KEY,
            root       TEXT,
            host       TEXT,
            up         TEXT,
            base_total INTEGER NOT NULL,
            base_done  INTEGER NOT NULL,
            base_tops  INTEGER NOT NULL,
            own_total  INTEGER NOT NULL,
            own_done   INTEGER NOT NULL,
            total      INTEGER NOT NULL,
            done       INTEGER NOT NULL,
            base_docs  INTEGER NOT NULL DEFAULT 0,
            own_docs   INTEGER NOT NULL DEFAULT 0,
            docs       INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS rollup_cross (
            id    TEXT PRIMARY KEY,
            total INTEGER NOT NULL,
            done  INTEGER NOT NULL,
            home  TEXT NOT NULL,
            docs  INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS rollup_marks (
            mark TEXT NOT NULL CHECK (mark IN ('cycle', 'dangling')),
            id   TEXT NOT NULL,
            PRIMARY KEY (mark, id)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS status_counts (
            epic   TEXT NOT NULL,
            status TEXT NOT NULL,
            held   INTEGER NOT NULL,
            PRIMARY KEY (epic, status)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS ready_counts (
            epic TEXT PRIMARY KEY,
            held INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS cells (
            id       INTEGER PRIMARY KEY,
            state    TEXT NOT NULL,
            kind     TEXT NOT NULL,
            severity TEXT,
            blocked  INTEGER NOT NULL,
            commits  INTEGER NOT NULL,
            menger   INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS cell_bits (
            epic TEXT NOT NULL,
            cell INTEGER NOT NULL,
            hi   INTEGER NOT NULL,
            card INTEGER NOT NULL,
            bits BLOB NOT NULL,
            PRIMARY KEY (epic, cell, hi)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS cell_counts (
            epic TEXT NOT NULL,
            cell INTEGER NOT NULL,
            held INTEGER NOT NULL,
            PRIMARY KEY (epic, cell)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS tree_tallies (
            parent TEXT NOT NULL,
            epic   TEXT NOT NULL,
            cell   INTEGER NOT NULL,
            held   INTEGER NOT NULL,
            PRIMARY KEY (parent, epic, cell)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS tree_holders (
            parent TEXT NOT NULL,
            kids   TEXT NOT NULL,
            epic   TEXT NOT NULL,
            cell   INTEGER NOT NULL,
            hi     INTEGER NOT NULL,
            card   INTEGER NOT NULL,
            bits   BLOB NOT NULL,
            PRIMARY KEY (parent, kids, epic, cell, hi)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS derived_state (
            one   INTEGER PRIMARY KEY CHECK (one = 1),
            form  INTEGER NOT NULL,
            stale INTEGER NOT NULL
        );
    ";

/// The children index: each parent's children in the store's order.
const CHILDREN_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS tickets_by_parent ON tickets (parent, epic, seq)";

/// SCALE-32: the ready set — MNG-4's rule, as the file can answer it: the
/// tickets `Ready` with no unsatisfied blocker, in the store's order. Partial,
/// so it holds the members and nothing else; a read names the same two
/// terms ([`read_ready`]) and SQLite answers from it.
const READY_INDEX: &str = "CREATE INDEX IF NOT EXISTS tickets_ready ON tickets (epic, seq)
         WHERE status = 'Ready' AND unsatisfied = 0";

/// The children index, built where it is missing — sorted, in one pass.
pub(super) fn create_children_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(CHILDREN_INDEX)
}

/// The children index, dropped — by a seed that marked the derived state
/// stale in the same transaction, and builds it again when it stores it.
pub(super) fn drop_children_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("DROP INDEX IF EXISTS tickets_by_parent")
}

/// SCALE-32: the ready set's index, built where it is missing — once the
/// counts it is over are written, so it is one sorted pass and not a row at
/// a time as each count lands.
pub(super) fn create_ready_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(READY_INDEX)
}

/// SCALE-32: the ready set's index, dropped — by a build or a large seed,
/// which write the counts it is over and then build it again.
pub(super) fn drop_ready_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("DROP INDEX IF EXISTS tickets_ready")
}

/// A shortcode's run as stored: `(first, last, held)` (SCALE-19's `Run`).
pub(super) type RunRow = (u32, u32, u64);

/// A dangling forward reference as stored: the id named and the naming
/// ticket's position — the key — and, as the value, the naming ticket's id.
pub(super) type RefKey = (String, OrderKey);

/// A shortcode's roll-up as stored: every field the roll-ups keep for it.
/// Each tally is `(total, done, docs)` (SDLC-11: `docs` since form 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GroupRow {
    pub(super) root: Option<String>,
    pub(super) host: Option<String>,
    pub(super) up: Option<String>,
    pub(super) base: (u64, u64, u64),
    pub(super) base_tops: u64,
    pub(super) own: (u64, u64, u64),
    pub(super) roll: (u64, u64, u64),
}

/// A cross top's tree as stored: its tally and the group it is drawn in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CrossRow {
    pub(super) tally: (u64, u64, u64),
    pub(super) home: String,
}

/// What a hand edit can leave that the roll-ups must remember (MNG-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Mark {
    /// A ticket on a parent cycle.
    Cycle,
    /// An id named as a parent that is no ticket.
    Dangling,
}

impl Mark {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cycle => "cycle",
            Self::Dangling => "dangling",
        }
    }

    fn parse(s: &str) -> rusqlite::Result<Self> {
        match s {
            "cycle" => Ok(Self::Cycle),
            "dangling" => Ok(Self::Dangling),
            other => Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("rollup_marks: unknown mark '{other}'").into(),
            )),
        }
    }
}

/// The derived state as numbers, every structure in one shape whether it
/// was read from the file or built by a scan — what a build writes, what
/// `--verify-derived` compares and what the tests hold to their oracles.
/// The children index itself is not here: it is the file's own index on
/// the tickets, read into a [`Children`] and compared as one. Nor is the
/// ready set (SCALE-32), for the same reason: it is read from its index
/// ([`read_ready`]) and compared as a set.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Form {
    pub(super) runs: BTreeMap<String, RunRow>,
    pub(super) refs: BTreeMap<RefKey, String>,
    pub(super) groups: BTreeMap<String, GroupRow>,
    pub(super) cross: BTreeMap<String, CrossRow>,
    pub(super) marks: BTreeSet<(Mark, String)>,
    /// Each ticket whose `child_count` is not 0, with it.
    pub(super) counts: BTreeMap<String, u64>,
    /// SCALE-32: each ticket whose count of unsatisfied blockers is not 0,
    /// with it.
    pub(super) unsatisfied: BTreeMap<String, u32>,
    /// SCALE-32: each shortcode's count of each status it holds.
    pub(super) statuses: BTreeMap<(String, &'static str), u64>,
    /// SCALE-32: each shortcode's share of the ready set, where it has one.
    pub(super) ready: BTreeMap<String, u64>,
    /// FILTER-4: the cell table, every container and every count, by the
    /// ids the cells were interned with.
    pub(super) cells: cells::Stored,
    /// FILTER-13: every tally entry and every holder container, by those ids.
    pub(super) tree: tree::Stored,
}

/// One write's change to the stored derived state — the rows its own
/// transaction writes beside the tickets, so the two commit together or not
/// at all. Worked out before the commit from the cache as it stands (under
/// the write-serial lock, nothing else moves it) and from nothing else.
#[derive(Debug, Default)]
pub(super) struct Changes {
    pub(super) runs: Vec<(String, RunRow)>,
    /// Ids born: every reference that named them leaves.
    pub(super) refs_born: Vec<String>,
    pub(super) refs_named: Vec<(RefKey, String)>,
    /// Each ticket whose `child_count` moves, with the count it moves to.
    pub(super) counts: Vec<(String, u64)>,
    /// `None`: the row goes.
    pub(super) groups: Vec<(String, Option<GroupRow>)>,
    pub(super) cross: Vec<(String, Option<CrossRow>)>,
    /// `true`: the mark is made; `false`: it goes.
    pub(super) marks: Vec<((Mark, String), bool)>,
    /// SCALE-32: each ticket whose count of unsatisfied blockers moves, with
    /// the count it moves to — and with it, by SQLite, the ready set's index.
    pub(super) unsatisfied: Vec<(String, u32)>,
    /// SCALE-32: each shortcode's count of a status that moves, with what it
    /// moves to; 0: the row goes.
    pub(super) statuses: Vec<((String, &'static str), u64)>,
    /// SCALE-32: each shortcode's share of the ready set that moves; 0: the
    /// row goes.
    pub(super) ready: Vec<(String, u64)>,
    /// FILTER-4: each cell interned by the write, with its id.
    pub(super) cells: Vec<(u32, Key)>,
    /// FILTER-4: each container a cell move rewrites, `(epic, cell, hi)`, as
    /// it is after the write — `None`: left empty, the row goes.
    pub(super) cell_bits: Vec<((String, u32, u32), Option<(u64, Vec<u8>)>)>,
    /// FILTER-4: each `(epic, cell)` count that moves; 0: the row goes.
    pub(super) cell_counts: Vec<((String, u32), u64)>,
    /// FILTER-13: the whole kept tree, written over what is stored — a move
    /// off a hand-edited cycle, which builds it again.
    pub(super) tree_all: Option<tree::Stored>,
    /// FILTER-13: each list whose stored holders all go, `(parent, kids)`,
    /// before any holder row is written.
    pub(super) tree_cleared: Vec<(String, String)>,
    /// FILTER-13: each holder container a write rewrites, as it is after —
    /// `None`: left empty, the row goes.
    pub(super) tree_holders: Vec<(tree::HolderKey, Option<(u64, Vec<u8>)>)>,
    /// FILTER-13: each tally entry that moves, `(parent, epic, cell)`, with
    /// its count after; 0: the row goes.
    pub(super) tree_tallies: Vec<((String, String, u32), u32)>,
}

impl Changes {
    /// Writes them, inside the caller's transaction.
    pub(super) fn write(&self, conn: &Connection) -> rusqlite::Result<()> {
        for (epic, run) in &self.runs {
            put_run(conn, epic, run)?;
        }
        for id in &self.refs_born {
            conn.prepare_cached("DELETE FROM forward_refs WHERE id = ?1")?
                .execute([id])?;
        }
        for (key, referrer) in &self.refs_named {
            put_ref(conn, key, referrer)?;
        }
        for (id, count) in &self.counts {
            put_count(conn, id, *count)?;
        }
        for (epic, row) in &self.groups {
            match row {
                Some(row) => put_group(conn, epic, row)?,
                None => {
                    conn.prepare_cached("DELETE FROM rollup_groups WHERE epic = ?1")?
                        .execute([epic])?;
                }
            }
        }
        for (id, row) in &self.cross {
            match row {
                Some(row) => put_cross(conn, id, row)?,
                None => {
                    conn.prepare_cached("DELETE FROM rollup_cross WHERE id = ?1")?
                        .execute([id])?;
                }
            }
        }
        for ((mark, id), made) in &self.marks {
            let sql = if *made {
                "INSERT INTO rollup_marks (mark, id) VALUES (?1, ?2)"
            } else {
                "DELETE FROM rollup_marks WHERE mark = ?1 AND id = ?2"
            };
            conn.prepare_cached(sql)?.execute(params![mark.as_str(), id])?;
        }
        // A crossing moves every count it moves to one of a few values
        // (most to 0, or to 1 and back): one statement per value, over the
        // ids as a list, rather than one per ticket.
        let mut by_value: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
        for (id, open) in &self.unsatisfied {
            by_value.entry(*open).or_default().push(id);
        }
        for (open, ids) in by_value {
            match ids.as_slice() {
                [id] => put_unsatisfied(conn, id, open)?,
                _ => {
                    let ids = serde_json::to_string(&ids).expect("serialize ids");
                    conn.prepare_cached(
                        "UPDATE tickets SET unsatisfied = ?1 WHERE id IN (SELECT value FROM json_each(?2))",
                    )?
                    .execute(params![open, ids])?;
                }
            }
        }
        for (key, held) in &self.statuses {
            put_status(conn, key, *held)?;
        }
        for (epic, held) in &self.ready {
            put_ready(conn, epic, *held)?;
        }
        for (id, key) in &self.cells {
            put_cell(conn, *id, key)?;
        }
        for (at, row) in &self.cell_bits {
            put_bits(conn, at, row.as_ref())?;
        }
        for (at, held) in &self.cell_counts {
            put_cell_count(conn, at, *held)?;
        }
        if let Some(all) = &self.tree_all {
            write_tree(conn, all)?;
        }
        for (parent, kids) in &self.tree_cleared {
            conn.prepare_cached("DELETE FROM tree_holders WHERE parent = ?1 AND kids = ?2")?
                .execute(params![parent, kids])?;
        }
        for (at, row) in &self.tree_holders {
            put_holder(conn, at, row.as_ref())?;
        }
        for (at, held) in &self.tree_tallies {
            put_tally(conn, at, *held)?;
        }
        Ok(())
    }
}

/// FILTER-13: one tally entry; 0 is no row.
fn put_tally(conn: &Connection, (parent, epic, cell): &(String, String, u32), held: u32) -> rusqlite::Result<()> {
    if held == 0 {
        conn.prepare_cached("DELETE FROM tree_tallies WHERE parent = ?1 AND epic = ?2 AND cell = ?3")?
            .execute(params![parent, epic, cell])?;
    } else {
        conn.prepare_cached(
            "INSERT INTO tree_tallies (parent, epic, cell, held) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (parent, epic, cell) DO UPDATE SET held = excluded.held",
        )?
        .execute(params![parent, epic, cell, held])?;
    }
    Ok(())
}

/// FILTER-13: one holder container as a write leaves it; `None` is no row.
fn put_holder(conn: &Connection, (parent, kids, epic, cell, hi): &tree::HolderKey, row: Option<&(u64, Vec<u8>)>) -> rusqlite::Result<()> {
    match row {
        None => {
            conn.prepare_cached(
                "DELETE FROM tree_holders WHERE parent = ?1 AND kids = ?2 AND epic = ?3 AND cell = ?4 AND hi = ?5",
            )?
            .execute(params![parent, kids, epic, cell, hi])?;
        }
        Some((card, bits)) => {
            conn.prepare_cached(
                "INSERT INTO tree_holders (parent, kids, epic, cell, hi, card, bits) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (parent, kids, epic, cell, hi) DO UPDATE SET card = excluded.card, bits = excluded.bits",
            )?
            .execute(params![parent, kids, epic, cell, hi, card, bits])?;
        }
    }
    Ok(())
}

/// FILTER-13: the kept tree written over whatever is stored.
fn write_tree(conn: &Connection, t: &tree::Stored) -> rusqlite::Result<()> {
    conn.execute_batch("DELETE FROM tree_tallies; DELETE FROM tree_holders;")?;
    for (at, held) in &t.tallies {
        put_tally(conn, at, *held)?;
    }
    for (at, row) in &t.holders {
        put_holder(conn, at, Some(row))?;
    }
    Ok(())
}

/// FILTER-4: one cell of the table.
fn put_cell(conn: &Connection, id: u32, key: &Key) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT INTO cells (id, state, kind, severity, blocked, commits, menger) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?
        .execute(params![
            id,
            key.status().as_str(),
            key.kind().as_str(),
            key.severity().map(|s| s.as_str()),
            key.blocked(),
            key.commits(),
            key.menger()
        ])?;
    Ok(())
}

/// FILTER-4: one container as a write leaves it; `None` is no row.
fn put_bits(conn: &Connection, (epic, cell, hi): &(String, u32, u32), row: Option<&(u64, Vec<u8>)>) -> rusqlite::Result<()> {
    match row {
        None => {
            conn.prepare_cached("DELETE FROM cell_bits WHERE epic = ?1 AND cell = ?2 AND hi = ?3")?
                .execute(params![epic, cell, hi])?;
        }
        Some((card, bits)) => {
            conn.prepare_cached(
                "INSERT INTO cell_bits (epic, cell, hi, card, bits) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (epic, cell, hi) DO UPDATE SET card = excluded.card, bits = excluded.bits",
            )?
            .execute(params![epic, cell, hi, card, bits])?;
        }
    }
    Ok(())
}

/// FILTER-4: an `(epic, cell)` count; 0 is no row.
fn put_cell_count(conn: &Connection, (epic, cell): &(String, u32), held: u64) -> rusqlite::Result<()> {
    if held == 0 {
        conn.prepare_cached("DELETE FROM cell_counts WHERE epic = ?1 AND cell = ?2")?
            .execute(params![epic, cell])?;
    } else {
        conn.prepare_cached(
            "INSERT INTO cell_counts (epic, cell, held) VALUES (?1, ?2, ?3)
                 ON CONFLICT (epic, cell) DO UPDATE SET held = excluded.held",
        )?
        .execute(params![epic, cell, held])?;
    }
    Ok(())
}

/// SCALE-32: one ticket's count of unsatisfied blockers — a birth's, or a
/// build's where the column differs from the scan.
fn put_unsatisfied(conn: &Connection, id: &str, open: u32) -> rusqlite::Result<()> {
    conn.prepare_cached("UPDATE tickets SET unsatisfied = ?2 WHERE id = ?1")?
        .execute(params![id, open])?;
    Ok(())
}

/// SCALE-32: a shortcode's count of a status; 0 is no row.
fn put_status(conn: &Connection, (epic, status): &(String, &'static str), held: u64) -> rusqlite::Result<()> {
    if held == 0 {
        conn.prepare_cached("DELETE FROM status_counts WHERE epic = ?1 AND status = ?2")?
            .execute(params![epic, status])?;
    } else {
        conn.prepare_cached(
            "INSERT INTO status_counts (epic, status, held) VALUES (?1, ?2, ?3)
                 ON CONFLICT (epic, status) DO UPDATE SET held = excluded.held",
        )?
        .execute(params![epic, status, held])?;
    }
    Ok(())
}

/// SCALE-32: a shortcode's share of the ready set; 0 is no row.
fn put_ready(conn: &Connection, epic: &str, held: u64) -> rusqlite::Result<()> {
    if held == 0 {
        conn.prepare_cached("DELETE FROM ready_counts WHERE epic = ?1")?.execute([epic])?;
    } else {
        conn.prepare_cached(
            "INSERT INTO ready_counts (epic, held) VALUES (?1, ?2)
                 ON CONFLICT (epic) DO UPDATE SET held = excluded.held",
        )?
        .execute(params![epic, held])?;
    }
    Ok(())
}

fn put_run(conn: &Connection, epic: &str, (first, last, held): &RunRow) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO shortcode_runs (epic, first, last, held) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (epic) DO UPDATE SET
                 first = excluded.first, last = excluded.last, held = excluded.held",
    )?
    .execute(params![epic, first, last, held])?;
    Ok(())
}

fn put_ref(conn: &Connection, (id, key): &RefKey, referrer: &str) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT INTO forward_refs (id, epic, seq, referrer) VALUES (?1, ?2, ?3, ?4)")?
        .execute(params![id, key.epic, key.seq, referrer])?;
    Ok(())
}

/// A ticket's `child_count`. Not checked for a row: a parent that names no
/// ticket (a hand edit) has none to carry a count, as in the cache.
fn put_count(conn: &Connection, id: &str, count: u64) -> rusqlite::Result<()> {
    conn.prepare_cached("UPDATE tickets SET child_count = ?2 WHERE id = ?1")?
        .execute(params![id, count])?;
    Ok(())
}

fn put_group(conn: &Connection, epic: &str, g: &GroupRow) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO rollup_groups
                 (epic, root, host, up, base_total, base_done, base_tops,
                  own_total, own_done, total, done, base_docs, own_docs, docs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (epic) DO UPDATE SET
                 root = excluded.root, host = excluded.host, up = excluded.up,
                 base_total = excluded.base_total, base_done = excluded.base_done,
                 base_tops = excluded.base_tops, own_total = excluded.own_total,
                 own_done = excluded.own_done, total = excluded.total, done = excluded.done,
                 base_docs = excluded.base_docs, own_docs = excluded.own_docs, docs = excluded.docs",
    )?
    .execute(params![
        epic, g.root, g.host, g.up, g.base.0, g.base.1, g.base_tops, g.own.0, g.own.1, g.roll.0, g.roll.1,
        g.base.2, g.own.2, g.roll.2,
    ])?;
    Ok(())
}

fn put_cross(conn: &Connection, id: &str, c: &CrossRow) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO rollup_cross (id, total, done, home, docs) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET
                 total = excluded.total, done = excluded.done, home = excluded.home,
                 docs = excluded.docs",
    )?
    .execute(params![id, c.tally.0, c.tally.1, c.home, c.tally.2])?;
    Ok(())
}

/// Where a file's stored derived state stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    /// Stored, in this build's form, and as new as the tickets.
    Current,
    /// None: the file was written before SCALE-28 (or is new).
    Absent,
    /// Stored in an older form.
    Older(i64),
    /// Behind the tickets: a seed committed tickets and stopped before
    /// their derived state.
    Stale,
}

pub(super) fn state(conn: &Connection) -> rusqlite::Result<State> {
    let table: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'derived_state'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )?;
    if !table {
        return Ok(State::Absent);
    }
    let row: Option<(i64, bool)> = conn
        .query_row("SELECT form, stale FROM derived_state WHERE one = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    Ok(match row {
        None => State::Absent,
        Some((_, true)) => State::Stale,
        Some((form, false)) if form < FORM => State::Older(form),
        Some(_) => State::Current,
    })
}

/// Says, in the caller's transaction, whether the stored derived state is
/// behind the tickets.
pub(super) fn mark_stale(conn: &Connection, stale: bool) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO derived_state (one, form, stale) VALUES (1, ?1, ?2)
             ON CONFLICT (one) DO UPDATE SET form = excluded.form, stale = excluded.stale",
    )?
    .execute(params![FORM, stale])?;
    Ok(())
}

/// SCALE-28: the derived state, built once from the tickets and stored —
/// for a file written before SCALE-28, one whose seed stopped before its
/// derived state was stored, or one holding an older form. The pattern of
/// the in-place migrations in `db.rs`: one transaction, before the cache is
/// handed out, logged. In it the file gains what the form needs (the
/// `child_count` column — `ADD COLUMN` with a constant default rewrites no
/// row — the tables and the children index); every ticket's row is read and
/// the four structures (SCALE-32: five) built from them by a scan, as every
/// start built them before SCALE-28 (and with the roll-ups' own rule for hand-edited data:
/// what is stored is what `rollup.rs` builds); and all of it is written
/// over whatever was there. A SIGKILL before the commit leaves the file as
/// it was, and the next open builds again; after it, every open reads.
///
/// Returns the cache it built — the tickets and their derived state, as
/// stored — so the open that built it does not read it all again.
///
/// SCALE-32: the file also gains the `unsatisfied` column (as `child_count`
/// was added) and the ready set's index — dropped first and built after the
/// counts are written, so it is one sorted pass.
pub(super) fn build(conn: &mut Connection, found: State) -> anyhow::Result<Cache> {
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;
    for column in ["child_count", "unsatisfied"] {
        if !db::has_column(&tx, "tickets", column)? {
            tx.execute(&format!("ALTER TABLE tickets ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"), [])?;
        }
    }
    tx.execute_batch(SCHEMA)?;
    // SDLC-11: a roll-up table from an older form gains its `docs` columns
    // (a new one was just created with them); `write_all` fills them.
    // FILTER-21: and the cell table from form 4 or 5 its `menger` column —
    // every cell is written again below, so none keeps the default.
    for (table, column) in DOCS_COLUMNS.into_iter().chain([MENGER_COLUMN]) {
        if !db::has_column(&tx, table, column)? {
            tx.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"), [])?;
        }
    }
    create_children_index(&tx)?;
    drop_ready_index(&tx)?;
    let cache = Cache::new(db::load_rows(&tx)?);
    let form = cache.form();
    write_all(&tx, &form)?;
    create_ready_index(&tx)?;
    mark_stale(&tx, false)?;
    tx.commit()?;
    let why = match found {
        State::Absent => "for the first time".to_string(),
        State::Stale => "again: it was behind the tickets (a seed stopped before storing it)".to_string(),
        State::Older(form) => format!("again: it was stored in form {form}, this build writes {FORM}"),
        State::Current => "again".to_string(),
    };
    let message = format!(
        "SCALE-28: stored the derived state {why} — {} shortcode runs, {} dangling forward \
         references, {} roll-up groups ({} cross trees, {} marks), the child_count of {} parents, \
         the unsatisfied blockers of {} tickets, a ready set of {}, the status counts of {} \
         shortcodes and {} cells' {} bitmaps (FILTER-4) — built once by a scan of {} tickets in \
         {:.1} ms, durably; every later open reads it",
        form.runs.len(),
        form.refs.len(),
        form.groups.len(),
        form.cross.len(),
        form.marks.len(),
        form.counts.len(),
        form.unsatisfied.len(),
        cache.ready.size(),
        cache.ready.shortcodes(),
        cache.cells.keys().len(),
        cache.cells.bitmaps(),
        cache.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    match found {
        State::Absent => tracing::info!("{message}"),
        _ => tracing::warn!("{message}"),
    }
    Ok(cache)
}

/// Every table set to `form`, in the caller's transaction: what is there
/// and differs from it is replaced, what it does not hold goes. The counts
/// are compared with the column first, so only a row whose count moves is
/// written — a scan of the column, which a build or a seed can afford.
/// SCALE-32: the same for the counts of unsatisfied blockers.
pub(super) fn write_all(conn: &Connection, form: &Form) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM shortcode_runs; DELETE FROM forward_refs; DELETE FROM rollup_groups;
         DELETE FROM rollup_cross; DELETE FROM rollup_marks; DELETE FROM status_counts;
         DELETE FROM ready_counts; DELETE FROM cells; DELETE FROM cell_bits; DELETE FROM cell_counts;",
    )?;
    for (id, key) in &form.cells.keys {
        put_cell(conn, *id, key)?;
    }
    for (at, row) in &form.cells.bits {
        put_bits(conn, at, Some(row))?;
    }
    for (at, held) in &form.cells.counts {
        put_cell_count(conn, at, *held)?;
    }
    write_tree(conn, &form.tree)?;
    for (key, held) in &form.statuses {
        put_status(conn, key, *held)?;
    }
    for (epic, held) in &form.ready {
        put_ready(conn, epic, *held)?;
    }
    let stored = read_unsatisfied(conn)?;
    for id in stored.keys().filter(|id| !form.unsatisfied.contains_key(*id)) {
        put_unsatisfied(conn, id, 0)?;
    }
    for (id, open) in form.unsatisfied.iter().filter(|(id, n)| stored.get(*id) != Some(*n)) {
        put_unsatisfied(conn, id, *open)?;
    }
    for (epic, run) in &form.runs {
        put_run(conn, epic, run)?;
    }
    for (key, referrer) in &form.refs {
        put_ref(conn, key, referrer)?;
    }
    for (epic, row) in &form.groups {
        put_group(conn, epic, row)?;
    }
    for (id, row) in &form.cross {
        put_cross(conn, id, row)?;
    }
    for (mark, id) in &form.marks {
        conn.prepare_cached("INSERT INTO rollup_marks (mark, id) VALUES (?1, ?2)")?
            .execute(params![mark.as_str(), id])?;
    }
    let stored = read_counts(conn)?;
    for id in stored.keys().filter(|id| !form.counts.contains_key(*id)) {
        put_count(conn, id, 0)?;
    }
    for (id, count) in form.counts.iter().filter(|(id, n)| stored.get(*id) != Some(*n)) {
        put_count(conn, id, *count)?;
    }
    Ok(())
}

fn read_counts(conn: &Connection) -> rusqlite::Result<BTreeMap<String, u64>> {
    let mut stmt = conn.prepare("SELECT id, child_count FROM tickets WHERE child_count <> 0")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// SCALE-32: every count of unsatisfied blockers that is not 0.
fn read_unsatisfied(conn: &Connection) -> rusqlite::Result<BTreeMap<String, u32>> {
    let mut stmt = conn.prepare("SELECT id, unsatisfied FROM tickets WHERE unsatisfied <> 0")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// Every table, as stored, with every `child_count` that is not 0 — the
/// form `--verify-derived` compares with a scan. The counts are a scan of
/// the column (it has no index, and needs none: a start has each with its
/// row, and B4 reads a row's when it reads the row). SCALE-32: and every
/// count of unsatisfied blockers, the same way.
pub(super) fn read(conn: &Connection) -> rusqlite::Result<Form> {
    let mut form = read_tables(conn)?;
    form.counts = read_counts(conn)?;
    form.unsatisfied = read_unsatisfied(conn)?;
    form.cells = read_cells(conn)?;
    form.tree = read_tree(conn)?;
    Ok(form)
}

/// FILTER-13: every tally entry and every holder container, as stored —
/// what `--verify-derived` compares.
fn read_tree(conn: &Connection) -> rusqlite::Result<tree::Stored> {
    let mut out = tree::Stored::default();
    let mut stmt = conn.prepare("SELECT parent, epic, cell, held FROM tree_tallies")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        out.tallies.insert((r.get(0)?, r.get(1)?, r.get(2)?), r.get(3)?);
    }
    let mut stmt = conn.prepare("SELECT parent, kids, epic, cell, hi, card, bits FROM tree_holders")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        out.holders.insert((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?), (r.get(5)?, r.get(6)?));
    }
    Ok(out)
}

/// FILTER-13: the kept tree as the cache holds it, from what is stored, into
/// the children index read just before it: each tally into its parent's
/// entry, each holder bitmap — its containers gathered, each checked — into
/// its list. A row whose parent has no children, or whose list is not one
/// kept as a bitmap, fails the open, naming it: the file is not what this
/// build wrote.
fn load_tree(conn: &Connection, children: &mut Children) -> anyhow::Result<(usize, usize)> {
    let mut stmt = conn.prepare("SELECT parent, epic, cell, held FROM tree_tallies ORDER BY parent, epic, cell")?;
    let mut rows = stmt.query([])?;
    let mut tallied = 0;
    let mut current: Option<(String, Vec<(u32, u32)>)> = None;
    let install = |children: &mut Children, (parent, mut t): (String, Vec<(u32, u32)>)| -> anyhow::Result<()> {
        t.sort_unstable();
        t.shrink_to_fit();
        let kids = children
            .kids_mut(&parent)
            .ok_or_else(|| anyhow::anyhow!("tree_tallies {parent}: no ticket names it as a parent"))?;
        kids.tally = t;
        Ok(())
    };
    while let Some(r) = rows.next()? {
        let parent = r.get_ref(0)?.as_str()?;
        let (epic, cell, held): (&str, u32, u32) = (r.get_ref(1)?.as_str()?, r.get(2)?, r.get(3)?);
        let gg = tree::g(children.names_mut().intern(epic), cell);
        match &mut current {
            Some((p, t)) if p == parent => t.push((gg, held)),
            _ => {
                if let Some(done) = current.take() {
                    install(children, done)?;
                }
                tallied += 1;
                current = Some((parent.to_string(), vec![(gg, held)]));
            }
        }
    }
    if let Some(done) = current {
        install(children, done)?;
    }
    let mut stmt = conn.prepare("SELECT parent, kids, epic, cell, hi, card, bits FROM tree_holders ORDER BY parent, kids, epic, cell, hi")?;
    let mut rows = stmt.query([])?;
    let mut bitmaps = 0;
    let mut current: Option<((String, String, u32), roaring::RoaringBitmap)> = None;
    let place = |children: &mut Children, ((parent, kids, gg), bm): ((String, String, u32), roaring::RoaringBitmap)| -> anyhow::Result<()> {
        let many = children
            .kids_mut(&parent)
            .and_then(|k| k.list_mut(&kids))
            .and_then(super::children::Seqs::many_mut)
            .ok_or_else(|| anyhow::anyhow!("tree_holders {parent} {kids}: not a list of more than 256 children"))?;
        many.holders.push((gg, bm));
        Ok(())
    };
    while let Some(r) = rows.next()? {
        let (parent, kids, epic) = (r.get_ref(0)?.as_str()?, r.get_ref(1)?.as_str()?, r.get_ref(2)?.as_str()?);
        let (cell, hi, card): (u32, u32, u64) = (r.get(3)?, r.get(4)?, r.get(5)?);
        let lows = cells::decode(card, r.get_ref(6)?.as_blob()?)
            .map_err(|e| anyhow::anyhow!("tree_holders {parent} {kids} {epic} cell {cell} container {hi}: {e}"))?;
        let gg = tree::g(children.names_mut().intern(epic), cell);
        let same = matches!(&current, Some(((p, k, x), _)) if p == parent && k == kids && *x == gg);
        if !same {
            if let Some(done) = current.take() {
                place(children, done)?;
            }
            bitmaps += 1;
            current = Some(((parent.to_string(), kids.to_string(), gg), roaring::RoaringBitmap::new()));
        }
        let bm = &mut current.as_mut().expect("set above").1;
        bm.extend(lows.into_iter().map(|low| (hi << 16) | u32::from(low)));
    }
    if let Some(done) = current {
        place(children, done)?;
    }
    for (_, kids) in children.every_mut() {
        for (_, seqs) in kids.lists_mut() {
            if let Some(many) = seqs.many_mut() {
                many.holders.sort_by_key(|(gg, _)| *gg);
            }
        }
    }
    Ok((tallied, bitmaps))
}

/// A status as stored, as the `&'static str` it is kept under.
fn status_str(stored: &str) -> rusqlite::Result<&'static str> {
    stored.parse::<super::Status>().map(super::Status::as_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, e.into())
    })
}

/// The tables alone — a few rows a shortcode — what a start reads its
/// structures from.
fn read_tables(conn: &Connection) -> rusqlite::Result<Form> {
    let mut form = Form::default();
    let mut stmt = conn.prepare("SELECT epic, first, last, held FROM shortcode_runs")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.runs.insert(r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?));
    }
    let mut stmt = conn.prepare("SELECT id, epic, seq, referrer FROM forward_refs")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let key = OrderKey {
            epic: r.get(1)?,
            seq: r.get(2)?,
        };
        form.refs.insert((r.get(0)?, key), r.get(3)?);
    }
    let mut stmt = conn.prepare(
        "SELECT epic, root, host, up, base_total, base_done, base_tops, own_total, own_done, total, done,
                base_docs, own_docs, docs
             FROM rollup_groups",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.groups.insert(
            r.get(0)?,
            GroupRow {
                root: r.get(1)?,
                host: r.get(2)?,
                up: r.get(3)?,
                base: (r.get(4)?, r.get(5)?, r.get(11)?),
                base_tops: r.get(6)?,
                own: (r.get(7)?, r.get(8)?, r.get(12)?),
                roll: (r.get(9)?, r.get(10)?, r.get(13)?),
            },
        );
    }
    let mut stmt = conn.prepare("SELECT id, total, done, home, docs FROM rollup_cross")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.cross.insert(
            r.get(0)?,
            CrossRow {
                tally: (r.get(1)?, r.get(2)?, r.get(4)?),
                home: r.get(3)?,
            },
        );
    }
    let mut stmt = conn.prepare("SELECT mark, id FROM rollup_marks")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.marks.insert((Mark::parse(&r.get::<_, String>(0)?)?, r.get(1)?));
    }
    let mut stmt = conn.prepare("SELECT epic, status, held FROM status_counts")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.statuses.insert((r.get(0)?, status_str(r.get_ref(1)?.as_str()?)?), r.get(2)?);
    }
    let mut stmt = conn.prepare("SELECT epic, held FROM ready_counts")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.ready.insert(r.get(0)?, r.get(1)?);
    }
    Ok(form)
}

/// A column that names one of a closed set, parsed — or the conversion
/// failure naming it.
fn parsed<T: std::str::FromStr<Err = String>>(column: usize, stored: &str) -> rusqlite::Result<T> {
    stored
        .parse()
        .map_err(|e: String| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, e.into()))
}

/// FILTER-4: the cell table, by id.
fn read_cell_table(conn: &Connection) -> rusqlite::Result<BTreeMap<u32, Key>> {
    let mut out = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT id, state, kind, severity, blocked, commits, menger FROM cells ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let severity = match r.get_ref(3)?.as_str_or_null()? {
            None => None,
            Some(s) => Some(parsed(3, s)?),
        };
        let key = Key::new(
            parsed(1, r.get_ref(1)?.as_str()?)?,
            parsed(2, r.get_ref(2)?.as_str()?)?,
            severity,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
        );
        out.insert(r.get(0)?, key);
    }
    Ok(out)
}

/// FILTER-4: the cell table, every container and every count, as stored —
/// what `--verify-derived` compares.
fn read_cells(conn: &Connection) -> rusqlite::Result<cells::Stored> {
    let mut out = cells::Stored {
        keys: read_cell_table(conn)?,
        ..cells::Stored::default()
    };
    let mut stmt = conn.prepare("SELECT epic, cell, hi, card, bits FROM cell_bits ORDER BY epic, cell, hi")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        out.bits.insert((r.get(0)?, r.get(1)?, r.get(2)?), (r.get(3)?, r.get(4)?));
    }
    let mut stmt = conn.prepare("SELECT epic, cell, held FROM cell_counts")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        out.counts.insert((r.get(0)?, r.get(1)?), r.get(2)?);
    }
    Ok(out)
}

/// FILTER-4: the cells as the cache holds them, from what is stored: the
/// table, and each container streamed into its bitmap ([`cells::Filling`])
/// — the counts are the bitmaps' lengths, so `cell_counts` is not read. A
/// container that cannot be one fails the open, naming it: the file is not
/// what this build wrote.
fn load_cells(conn: &Connection) -> anyhow::Result<Cells> {
    let keys = read_cell_table(conn)?;
    let mut filling = cells::Filling::default();
    let mut stmt = conn.prepare("SELECT epic, cell, hi, card, bits FROM cell_bits ORDER BY epic, cell, hi")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        filling
            .push(r.get_ref(0)?.as_str()?, r.get(1)?, r.get(2)?, r.get(3)?, r.get_ref(4)?.as_blob()?)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    let held = filling.finish().map_err(|e| anyhow::anyhow!("{e}"))?;
    Cells::from_stored(keys, held).map_err(|e| anyhow::anyhow!("{e}"))
}

/// SCALE-32: the ready set, from its index — in the store's order, so each
/// shortcode's members arrive together and ascending. The query names the
/// index's own two terms, which is what lets SQLite answer from it; `INDEXED
/// BY` makes that the index or an error, never a scan of the tickets. What a
/// start reads the ready set from, and what B4's `ready_set` is a range of.
pub(super) fn read_ready(conn: &Connection) -> rusqlite::Result<BTreeMap<String, BTreeSet<u32>>> {
    let mut stmt = conn.prepare(
        "SELECT epic, seq FROM tickets INDEXED BY tickets_ready
             WHERE status = 'Ready' AND unsatisfied = 0 ORDER BY epic, seq",
    )?;
    let mut rows = stmt.query([])?;
    let mut members: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    let mut here: Option<(String, BTreeSet<u32>)> = None;
    while let Some(r) = rows.next()? {
        let (epic, seq) = (r.get_ref(0)?.as_str()?, r.get(1)?);
        match &mut here {
            Some((e, seqs)) if e == epic => {
                seqs.insert(seq);
            }
            _ => {
                if let Some((e, seqs)) = here.take() {
                    members.insert(e, seqs);
                }
                here = Some((epic.to_string(), BTreeSet::from([seq])));
            }
        }
    }
    if let Some((e, seqs)) = here {
        members.insert(e, seqs);
    }
    Ok(members)
}

/// Every parent's children from the file's index on `(parent, epic, seq)`
/// — read in that order, so each parent's run of children arrives whole
/// and in the store's order, and the in-RAM index is built by pushing.
/// `INDEXED BY`: the index or an error, never a scan and a sort.
pub(super) fn read_children(conn: &Connection) -> rusqlite::Result<Children> {
    let mut stmt = conn.prepare(
        "SELECT parent, epic, seq FROM tickets INDEXED BY tickets_by_parent
             WHERE parent IS NOT NULL ORDER BY parent, epic, seq",
    )?;
    let mut rows = stmt.query([])?;
    let mut children = Children::filling();
    while let Some(r) = rows.next()? {
        children.push(r.get_ref(0)?.as_str()?, r.get_ref(1)?.as_str()?, r.get(2)?);
    }
    Ok(children)
}

/// The cache at start, from the rows and what is stored beside them — the
/// four structures (SCALE-32: five) read, none rebuilt: no pass over the tickets computes
/// them. Each ticket's `child_count` came with its row. SCALE-32: and its
/// count of unsatisfied blockers, `unsatisfied` (those not 0); the ready set
/// is read from its index, the status counts from their table.
pub(super) fn load(
    conn: &Connection,
    rows: HashMap<String, Row>,
    unsatisfied: HashMap<String, u32>,
) -> anyhow::Result<Cache> {
    let started = std::time::Instant::now();
    let form = read_tables(conn)?;
    let tables = started.elapsed();
    let mut children = read_children(conn)?;
    let indexed = started.elapsed();
    let (tallied, holders) = load_tree(conn, &mut children)?;
    let treed = started.elapsed();
    let members = read_ready(conn)?;
    let readied = started.elapsed();
    let cells = load_cells(conn)?;
    let (runs, refs, groups, cross, marks) =
        (form.runs.len(), form.refs.len(), form.groups.len(), form.cross.len(), form.marks.len());
    let parents = children.parents();
    let ids = Ids::from_stored(form.runs, form.refs);
    let rollups = Rollups::from_stored(form.groups, form.cross, form.marks);
    let ready = Ready::from_stored(unsatisfied, members, form.statuses);
    tracing::info!(
        "read the derived state from the database in {:.1} ms — {runs} shortcode runs, {refs} \
         dangling forward references, {groups} roll-up groups ({cross} cross trees, {marks} marks) \
         and the status counts of {} shortcodes in {:.1} ms, the children of {parents} parents \
         from the index in {:.1} ms, a ready set of {} from its index in {:.1} ms (and the \
         unsatisfied blockers of {} tickets with their rows), {} cells' {} bitmaps of {} \
         tickets from their containers (with the tables), and the kept tree's tallies of \
         {tallied} parents and {holders} holder bitmaps in {:.1} ms (FILTER-13); nothing rebuilt",
        started.elapsed().as_secs_f64() * 1000.0,
        ready.shortcodes(),
        tables.as_secs_f64() * 1000.0,
        (indexed - tables).as_secs_f64() * 1000.0,
        ready.size(),
        (readied - treed).as_secs_f64() * 1000.0,
        ready.blocked(),
        cells.keys().len(),
        cells.bitmaps(),
        cells.tickets(),
        (treed - indexed).as_secs_f64() * 1000.0,
    );
    Ok(Cache::from_parts(rows, ids, rollups, children, ready, cells))
}

/// What `menger --verify-derived` found.
#[derive(Debug)]
pub enum Verdict {
    /// The stored derived state is what a scan of the stored tickets gives.
    Consistent {
        tickets: usize,
        runs: usize,
        refs: usize,
        groups: usize,
        parents: usize,
        /// SCALE-32: tickets with an unsatisfied blocker.
        blocked: usize,
        /// SCALE-32: the ready set.
        ready: usize,
        /// FILTER-4: the cells occupied, and their (shortcode, cell) bitmaps.
        cells: usize,
        bitmaps: usize,
        /// FILTER-13: the tally entries of the kept tree, and its holder
        /// containers.
        tallies: usize,
        holders: usize,
    },
    /// Where they differ, one line each, naming the structure and the key.
    Differs(Vec<String>),
    /// Nothing to compare: the file holds no derived state of this form, or
    /// says it is behind its tickets. The next open by a server builds it.
    NotStored(String),
}

/// SCALE-28: `menger --verify-derived` — opens the database read-only, in
/// one read transaction (one snapshot), builds the four structures (SCALE-32:
/// five — the ready set compared with its index) by a scan of its tickets
/// exactly as a build does, reads what is stored, and
/// compares them, structure by structure and key by key. It writes nothing:
/// the connection cannot, and runs no migration, so a file that needs one
/// is reported, not changed.
pub fn verify(path: &Path) -> anyhow::Result<Verdict> {
    if !path.is_file() {
        anyhow::bail!("no database at {}", path.display());
    }
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let tx = conn.transaction()?;
    match state(&tx)? {
        State::Current => {}
        State::Absent => {
            return Ok(Verdict::NotStored(
                "no stored derived state: the file was written before SCALE-28 and no server has \
                 opened it since (the first open builds it)"
                    .into(),
            ))
        }
        State::Older(form) => {
            return Ok(Verdict::NotStored(format!(
                "the derived state is stored in form {form}; this build reads {FORM} (the next open \
                 builds it again)"
            )))
        }
        State::Stale => {
            return Ok(Verdict::NotStored(
                "the stored derived state says it is behind the tickets: a seed stopped before \
                 storing it (the next open builds it again)"
                    .into(),
            ))
        }
    }
    let scanned = Cache::new(db::load_rows(&tx)?);
    let stored = read(&tx)?;
    let stored_children = read_children(&tx)?;
    let stored_ready = read_ready(&tx)?;
    let want = scanned.form();
    let mut out = compare(&stored, &want);
    for parent in scanned.children.differences(&stored_children) {
        out.push(format!(
            "children of {parent}: the index holds {:?}; a scan gives {:?}",
            stored_children.of(&parent),
            scanned.children.of(&parent)
        ));
    }
    out.extend(ready_differences(&stored_ready, scanned.ready.members()));
    tx.commit()?;
    if !out.is_empty() {
        return Ok(Verdict::Differs(out));
    }
    Ok(Verdict::Consistent {
        tickets: scanned.len(),
        runs: want.runs.len(),
        refs: want.refs.len(),
        groups: want.groups.len(),
        parents: scanned.children.parents(),
        blocked: want.unsatisfied.len(),
        ready: scanned.ready.size(),
        cells: scanned.cells.keys().len(),
        bitmaps: scanned.cells.bitmaps(),
        tallies: want.tree.tallies.len(),
        holders: want.tree.holders.len(),
    })
}

/// SCALE-32: each ticket the ready set's index holds and a scan does not, or
/// a scan holds and the index does not — one line each, in the store's order.
pub(super) fn ready_differences(
    stored: &BTreeMap<String, BTreeSet<u32>>,
    scanned: &BTreeMap<String, BTreeSet<u32>>,
) -> Vec<String> {
    let all = |m: &BTreeMap<String, BTreeSet<u32>>| -> BTreeSet<(String, u32)> {
        m.iter().flat_map(|(e, seqs)| seqs.iter().map(move |s| (e.clone(), *s))).collect()
    };
    let (stored, scanned) = (all(stored), all(scanned));
    let mut out: Vec<(&(String, u32), &str)> = stored
        .difference(&scanned)
        .map(|k| (k, "the index holds it; a scan does not"))
        .chain(scanned.difference(&stored).map(|k| (k, "a scan holds it; the index does not")))
        .collect();
    out.sort();
    out.into_iter()
        .map(|((epic, seq), why)| format!("ready set {epic}-{seq}: {why}"))
        .collect()
}

/// Each place `stored` and `scanned` differ, one line each: the structure,
/// the key, what is stored and what a scan gives.
pub(super) fn compare(stored: &Form, scanned: &Form) -> Vec<String> {
    let mut out = Vec::new();
    diff(&mut out, "shortcode run", &stored.runs, &scanned.runs, |(first, last, held)| {
        format!("first {first}, last {last}, holding {held}")
    });
    let refs = |m: &BTreeMap<RefKey, String>| -> BTreeMap<String, String> {
        m.iter()
            .map(|((id, key), referrer)| (format!("{id} named by {referrer}"), format!("{}-{}", key.epic, key.seq)))
            .collect()
    };
    diff(&mut out, "dangling forward reference", &refs(&stored.refs), &refs(&scanned.refs), |at| {
        format!("at {at}")
    });
    diff(&mut out, "roll-up group", &stored.groups, &scanned.groups, |g| format!("{g:?}"));
    diff(&mut out, "roll-up cross tree", &stored.cross, &scanned.cross, |c| format!("{c:?}"));
    let marks = |s: &BTreeSet<(Mark, String)>| -> BTreeMap<String, ()> {
        s.iter().map(|(m, id)| (format!("{} {id}", m.as_str()), ())).collect()
    };
    diff(&mut out, "roll-up mark", &marks(&stored.marks), &marks(&scanned.marks), |()| "made".into());
    diff(&mut out, "child_count of", &stored.counts, &scanned.counts, |n| n.to_string());
    diff(&mut out, "unsatisfied blockers of", &stored.unsatisfied, &scanned.unsatisfied, |n| n.to_string());
    let statuses = |m: &BTreeMap<(String, &'static str), u64>| -> BTreeMap<String, u64> {
        m.iter().map(|((epic, status), n)| (format!("{epic} {status}"), *n)).collect()
    };
    diff(&mut out, "status count", &statuses(&stored.statuses), &statuses(&scanned.statuses), |n| n.to_string());
    diff(&mut out, "ready count", &stored.ready, &scanned.ready, |n| n.to_string());
    out.extend(cell_differences(&stored.cells, &scanned.cells));
    // FILTER-13: the kept tree, each cell named as what it is on its side.
    out.extend(tree::differences(
        &stored.tree,
        &scanned.tree,
        |c| stored.cells.keys.get(&c).map(Key::to_string),
        |c| scanned.cells.keys.get(&c).map(Key::to_string),
    ));
    out
}

/// FILTER-4: where the stored cells and a scan's differ. The two intern in
/// different orders, so they are compared by cell, not by id: each
/// container and each count keyed `(shortcode, cell, block)`, the stored
/// row's id named beside its cell. And the table itself must be one: every
/// cell once, every id a container or count names in it.
pub(super) fn cell_differences(stored: &cells::Stored, scanned: &cells::Stored) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: BTreeMap<Key, u32> = BTreeMap::new();
    for (id, key) in &stored.keys {
        if let Some(first) = seen.insert(*key, *id) {
            out.push(format!("cells {first} and {id}: both are {key}"));
        }
    }
    let name = |keys: &BTreeMap<u32, Key>, cell: u32| -> Result<Key, String> {
        keys.get(&cell).copied().ok_or_else(|| format!("cell {cell}, which the cell table does not hold"))
    };
    // (epic, cell, block) → what the row says, and the id it is under.
    let mut bits: BTreeMap<(String, Key, u32), ((u64, &[u8]), u32)> = BTreeMap::new();
    for ((epic, cell, hi), (card, raw)) in &stored.bits {
        match name(&stored.keys, *cell) {
            Ok(key) => {
                bits.insert((epic.clone(), key, *hi), ((*card, raw.as_slice()), *cell));
            }
            Err(e) => out.push(format!("cell_bits {epic} {e}, container {hi}: stored {card} members")),
        }
    }
    let mut want: BTreeMap<(String, Key, u32), (u64, &[u8])> = BTreeMap::new();
    for ((epic, cell, hi), (card, raw)) in &scanned.bits {
        want.insert((epic.clone(), scanned.keys[cell], *hi), (*card, raw.as_slice()));
    }
    let show = |(card, raw): (u64, &[u8])| match cells::decode(card, raw) {
        Ok(lows) if lows.len() <= 8 => format!("{card} members {lows:?}"),
        Ok(_) => format!("{card} members"),
        Err(e) => format!("{card} members, unreadable: {e}"),
    };
    for ((epic, key, hi), (row, id)) in &bits {
        match want.get(&(epic.clone(), *key, *hi)) {
            Some(w) if *w == *row => {}
            Some(w) => out.push(format!(
                "cell_bits {epic} cell {id} {key} container {hi}: stored {}; a scan gives {}",
                show(*row),
                show(*w)
            )),
            None => out.push(format!("cell_bits {epic} cell {id} {key} container {hi}: stored {}; a scan gives none", show(*row))),
        }
    }
    for ((epic, key, hi), w) in &want {
        if !bits.contains_key(&(epic.clone(), *key, *hi)) {
            out.push(format!("cell_bits {epic} {key} container {hi}: none stored; a scan gives {}", show(*w)));
        }
    }
    let mut counts: BTreeMap<(String, Key), (u64, u32)> = BTreeMap::new();
    for ((epic, cell), held) in &stored.counts {
        match name(&stored.keys, *cell) {
            Ok(key) => {
                counts.insert((epic.clone(), key), (*held, *cell));
            }
            Err(e) => out.push(format!("cell_counts {epic} {e}: stored {held}")),
        }
    }
    let want: BTreeMap<(String, Key), u64> =
        scanned.counts.iter().map(|((epic, cell), n)| ((epic.clone(), scanned.keys[cell]), *n)).collect();
    for ((epic, key), (held, id)) in &counts {
        match want.get(&(epic.clone(), *key)) {
            Some(w) if w == held => {}
            Some(w) => out.push(format!("cell_counts {epic} cell {id} {key}: stored {held}; a scan gives {w}")),
            None => out.push(format!("cell_counts {epic} cell {id} {key}: stored {held}; a scan gives none")),
        }
    }
    for ((epic, key), w) in &want {
        if !counts.contains_key(&(epic.clone(), *key)) {
            out.push(format!("cell_counts {epic} {key}: none stored; a scan gives {w}"));
        }
    }
    out
}

fn diff<V: PartialEq>(
    out: &mut Vec<String>,
    what: &str,
    stored: &BTreeMap<String, V>,
    scanned: &BTreeMap<String, V>,
    show: impl Fn(&V) -> String,
) {
    for (key, s) in stored {
        match scanned.get(key) {
            Some(w) if w == s => {}
            Some(w) => out.push(format!("{what} {key}: stored {}; a scan gives {}", show(s), show(w))),
            None => out.push(format!("{what} {key}: stored {}; a scan gives none", show(s))),
        }
    }
    for (key, w) in scanned.iter().filter(|(k, _)| !stored.contains_key(*k)) {
        out.push(format!("{what} {key}: none stored; a scan gives {}", show(w)));
    }
}
