use super::{children::Children, db, ids::Ids, rollup::Rollups, Cache, OrderKey, Row};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// Which form of the stored derived state this build writes and reads.
/// One table more (B3's edges, say) is a new form: a file holding an older
/// one is built again, once, as a file from before SCALE-28 is.
const FORM: i64 = 1;

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
///   nested group, the group it is drawn inside).
/// - `rollup_cross` — the tally of each tree whose top hangs in another
///   shortcode (every sub-shortcode's root), and the group it is drawn in.
/// - `rollup_marks` — what only a hand edit makes: tickets on a parent cycle
///   and ids named as a parent that are no ticket (MNG-17).
/// - `tickets_by_parent` ([`CHILDREN_INDEX`]) — a ticket's children in the
///   store's order: a page of `list { parent }` is a range of it (SCALE-4);
///   and each ticket's own `child_count` is a column on its row (added
///   beside it).
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
            done       INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS rollup_cross (
            id    TEXT PRIMARY KEY,
            total INTEGER NOT NULL,
            done  INTEGER NOT NULL,
            home  TEXT NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS rollup_marks (
            mark TEXT NOT NULL CHECK (mark IN ('cycle', 'dangling')),
            id   TEXT NOT NULL,
            PRIMARY KEY (mark, id)
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

/// The children index, built where it is missing — sorted, in one pass.
pub(super) fn create_children_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(CHILDREN_INDEX)
}

/// The children index, dropped — by a seed that marked the derived state
/// stale in the same transaction, and builds it again when it stores it.
pub(super) fn drop_children_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("DROP INDEX IF EXISTS tickets_by_parent")
}

/// A shortcode's run as stored: `(first, last, held)` (SCALE-19's `Run`).
pub(super) type RunRow = (u32, u32, u64);

/// A dangling forward reference as stored: the id named and the naming
/// ticket's position — the key — and, as the value, the naming ticket's id.
pub(super) type RefKey = (String, OrderKey);

/// A shortcode's roll-up as stored: every field the roll-ups keep for it.
/// Each tally is `(total, done)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GroupRow {
    pub(super) root: Option<String>,
    pub(super) host: Option<String>,
    pub(super) up: Option<String>,
    pub(super) base: (u64, u64),
    pub(super) base_tops: u64,
    pub(super) own: (u64, u64),
    pub(super) roll: (u64, u64),
}

/// A cross top's tree as stored: its tally and the group it is drawn in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CrossRow {
    pub(super) tally: (u64, u64),
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
/// the tickets, read into a [`Children`] and compared as one.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Form {
    pub(super) runs: BTreeMap<String, RunRow>,
    pub(super) refs: BTreeMap<RefKey, String>,
    pub(super) groups: BTreeMap<String, GroupRow>,
    pub(super) cross: BTreeMap<String, CrossRow>,
    pub(super) marks: BTreeSet<(Mark, String)>,
    /// Each ticket whose `child_count` is not 0, with it.
    pub(super) counts: BTreeMap<String, u64>,
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
        Ok(())
    }
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
                  own_total, own_done, total, done)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (epic) DO UPDATE SET
                 root = excluded.root, host = excluded.host, up = excluded.up,
                 base_total = excluded.base_total, base_done = excluded.base_done,
                 base_tops = excluded.base_tops, own_total = excluded.own_total,
                 own_done = excluded.own_done, total = excluded.total, done = excluded.done",
    )?
    .execute(params![
        epic, g.root, g.host, g.up, g.base.0, g.base.1, g.base_tops, g.own.0, g.own.1, g.roll.0, g.roll.1,
    ])?;
    Ok(())
}

fn put_cross(conn: &Connection, id: &str, c: &CrossRow) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO rollup_cross (id, total, done, home) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET
                 total = excluded.total, done = excluded.done, home = excluded.home",
    )?
    .execute(params![id, c.tally.0, c.tally.1, c.home])?;
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
/// the four structures built from them by a scan, as every start built them
/// before SCALE-28 (and with the roll-ups' own rule for hand-edited data:
/// what is stored is what `rollup.rs` builds); and all of it is written
/// over whatever was there. A SIGKILL before the commit leaves the file as
/// it was, and the next open builds again; after it, every open reads.
///
/// Returns the cache it built — the tickets and their derived state, as
/// stored — so the open that built it does not read it all again.
pub(super) fn build(conn: &mut Connection, found: State) -> anyhow::Result<Cache> {
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;
    if !db::has_column(&tx, "tickets", "child_count")? {
        tx.execute("ALTER TABLE tickets ADD COLUMN child_count INTEGER NOT NULL DEFAULT 0", [])?;
    }
    tx.execute_batch(SCHEMA)?;
    create_children_index(&tx)?;
    let cache = Cache::new(db::load_rows(&tx)?);
    let form = cache.form();
    write_all(&tx, &form)?;
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
         references, {} roll-up groups ({} cross trees, {} marks), the child_count of {} parents — \
         built once by a scan of {} tickets in {:.1} ms, durably; every later open reads it",
        form.runs.len(),
        form.refs.len(),
        form.groups.len(),
        form.cross.len(),
        form.marks.len(),
        form.counts.len(),
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
pub(super) fn write_all(conn: &Connection, form: &Form) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM shortcode_runs; DELETE FROM forward_refs; DELETE FROM rollup_groups;
         DELETE FROM rollup_cross; DELETE FROM rollup_marks;",
    )?;
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

/// Every table, as stored, with every `child_count` that is not 0 — the
/// form `--verify-derived` compares with a scan. The counts are a scan of
/// the column (it has no index, and needs none: a start has each with its
/// row, and B4 reads a row's when it reads the row).
pub(super) fn read(conn: &Connection) -> rusqlite::Result<Form> {
    let mut form = read_tables(conn)?;
    form.counts = read_counts(conn)?;
    Ok(form)
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
        "SELECT epic, root, host, up, base_total, base_done, base_tops, own_total, own_done, total, done
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
                base: (r.get(4)?, r.get(5)?),
                base_tops: r.get(6)?,
                own: (r.get(7)?, r.get(8)?),
                roll: (r.get(9)?, r.get(10)?),
            },
        );
    }
    let mut stmt = conn.prepare("SELECT id, total, done, home FROM rollup_cross")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.cross.insert(
            r.get(0)?,
            CrossRow {
                tally: (r.get(1)?, r.get(2)?),
                home: r.get(3)?,
            },
        );
    }
    let mut stmt = conn.prepare("SELECT mark, id FROM rollup_marks")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        form.marks.insert((Mark::parse(&r.get::<_, String>(0)?)?, r.get(1)?));
    }
    Ok(form)
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
/// four structures read, none rebuilt: no pass over the tickets computes
/// them. Each ticket's `child_count` came with its row.
pub(super) fn load(conn: &Connection, rows: HashMap<String, Row>) -> anyhow::Result<Cache> {
    let started = std::time::Instant::now();
    let form = read_tables(conn)?;
    let tables = started.elapsed();
    let children = read_children(conn)?;
    let (runs, refs, groups, cross, marks) =
        (form.runs.len(), form.refs.len(), form.groups.len(), form.cross.len(), form.marks.len());
    let parents = children.parents();
    let ids = Ids::from_stored(form.runs, form.refs);
    let rollups = Rollups::from_stored(form.groups, form.cross, form.marks);
    tracing::info!(
        "read the derived state from the database in {:.1} ms — {runs} shortcode runs, {refs} \
         dangling forward references, {groups} roll-up groups ({cross} cross trees, {marks} marks) \
         in {:.1} ms, the children of {parents} parents from the index in {:.1} ms; nothing rebuilt",
        started.elapsed().as_secs_f64() * 1000.0,
        tables.as_secs_f64() * 1000.0,
        (started.elapsed() - tables).as_secs_f64() * 1000.0
    );
    Ok(Cache::from_parts(rows, ids, rollups, children))
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
    },
    /// Where they differ, one line each, naming the structure and the key.
    Differs(Vec<String>),
    /// Nothing to compare: the file holds no derived state of this form, or
    /// says it is behind its tickets. The next open by a server builds it.
    NotStored(String),
}

/// SCALE-28: `menger --verify-derived` — opens the database read-only, in
/// one read transaction (one snapshot), builds the four structures by a scan
/// of its tickets exactly as a build does, reads what is stored, and
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
    let want = scanned.form();
    let mut out = compare(&stored, &want);
    for parent in scanned.children.differences(&stored_children) {
        out.push(format!(
            "children of {parent}: the index holds {:?}; a scan gives {:?}",
            stored_children.of(&parent),
            scanned.children.of(&parent)
        ));
    }
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
    })
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
