//! PERF-24: **bulk writes for the scale harness** (`scripts/scale/`) —
//! measurement only, behind the `scale` feature, which nothing that serves
//! enables.
//!
//! A dataset of 10³ … 10⁹ tickets is written straight into a store file
//! through the store's own pieces: the file is made by [`Store::open`] (every
//! table, index and migration a real start makes), the rows go in through
//! [`db::insert_many`] and the search index through [`search::put_many`] —
//! the seed's own path (PERF-2) — chunk by chunk, each chunk one
//! transaction that also marks the derived state stale ([`stored::mark_stale`]).
//! The next [`Store::open`] then builds the derived state from the tickets, as
//! it does for any seed that stopped before storing it — so the file the
//! harness measures is one menger made, not a copy of its schema.
//!
//! Unlike `seed_synthetic`, nothing is held in memory beyond one chunk, and a
//! file is extended in place: a decade is the one before it plus 9×.

use super::*;

/// A store file open for bulk appends.
pub struct Bulk {
    conn: Connection,
}

impl Bulk {
    /// The store file at `path`, made by a real open when absent. While the
    /// bulk writer holds it the journal is off and nothing is synced — a
    /// crash mid-generation loses the dataset, never anything served; and
    /// the children and ready indexes are dropped, as a large seed drops
    /// them, for the next open's build to make again in one sorted pass.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            drop(Store::open(path)?);
        }
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode = OFF", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA synchronous = OFF; PRAGMA cache_size = -2000000; PRAGMA temp_store = MEMORY;")?;
        {
            stored::drop_children_index(&conn)?;
            stored::drop_ready_index(&conn)?;
        }
        Ok(Self { conn })
    }

    /// The file at `path`, read only: for counting, never for appends.
    pub fn open_read(path: &Path) -> anyhow::Result<Self> {
        Ok(Self { conn: Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)? })
    }

    /// How many tickets the file holds.
    pub fn count(&self) -> anyhow::Result<u64> {
        Ok(self.conn.query_row("SELECT count(*) FROM tickets", [], |r| r.get::<_, i64>(0))? as u64)
    }

    /// Each shortcode's highest seq, as the file holds them.
    pub fn last_seqs(&self) -> anyhow::Result<HashMap<String, u32>> {
        let mut stmt = self.conn.prepare("SELECT epic, max(seq) FROM tickets GROUP BY epic")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `tickets` written in one transaction: the rows (each with no
    /// unsatisfied blocker counted — the build counts them) and their search
    /// rows, the derived state marked behind them.
    pub fn append(&mut self, tickets: &[Ticket]) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        stored::mark_stale(&tx, true)?;
        let open = vec![0u32; tickets.len()];
        db::insert_many(&tx, tickets, &open)?;
        search::put_many(&tx, tickets)?;
        tx.commit()?;
        Ok(())
    }

    /// The journal back to WAL, as [`Store::open`] keeps it.
    pub fn close(self) -> anyhow::Result<()> {
        self.conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        Ok(())
    }
}

/// One ticket the harness calls by name: its id, uid, kind and shortcode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sampled {
    pub id: String,
    pub uid: String,
    pub kind: String,
    pub epic: String,
}

/// `k` tickets of the file at `path`, drawn by rowid with `draw` — read
/// only, before a server opens it.
pub fn sample(path: &Path, k: usize, mut draw: impl FnMut(u64) -> u64) -> anyhow::Result<Vec<Sampled>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let max: i64 = conn.query_row("SELECT coalesce(max(rowid), 0) FROM tickets", [], |r| r.get(0))?;
    let mut stmt = conn.prepare("SELECT id, coalesce(uid, ''), kind, epic FROM tickets WHERE rowid >= ?1 ORDER BY rowid LIMIT 1")?;
    let mut out = Vec::with_capacity(k);
    while out.len() < k && max > 0 {
        let at = 1 + draw(max as u64) as i64;
        if let Some(s) = stmt
            .query_row([at], |r| Ok(Sampled { id: r.get(0)?, uid: r.get(1)?, kind: r.get(2)?, epic: r.get(3)? }))
            .optional_row()?
        {
            out.push(s);
        }
    }
    Ok(out)
}

/// The file's bytes by what holds them — `(name, bytes)` for each table and
/// index, from SQLite's `dbstat` (read only).
pub fn sizes(path: &Path) -> anyhow::Result<Vec<(String, u64)>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare("SELECT name, sum(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

trait OptionalRow<T> {
    fn optional_row(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional_row(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
