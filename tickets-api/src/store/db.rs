use super::{Row, Ticket};
use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

pub(super) const SCHEMA: &str = "
        CREATE TABLE IF NOT EXISTS tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            kind         TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            parent       TEXT,
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            commits      TEXT NOT NULL DEFAULT '[]',
            child_count  INTEGER NOT NULL DEFAULT 0,
            unsatisfied  INTEGER NOT NULL DEFAULT 0,
            path         TEXT,
            UNIQUE (epic, seq)
        );
    ";

/// WAVE-10: a path is unique within its shortcode — the file's own guard
/// under the cache's ([`super::Cache::paths`]). Partial: only the few
/// tickets that carry a path are in it.
const PATH_INDEX: &str =
    "CREATE UNIQUE INDEX IF NOT EXISTS tickets_path ON tickets (epic, path) WHERE path IS NOT NULL";

/// The pre-AGS-13 column, and its vocabulary mapped onto kinds (the
/// pinned migration). This is the only place the legacy vocabulary
/// survives: it exists to be read once and rewritten, never written.
const LEGACY_COLUMN: &str = "ticket_type";

fn legacy_kind(ticket_type: &str) -> Option<&'static str> {
    match ticket_type {
        "epic" => Some("scope"),
        "implementation" => Some("build"),
        "analysis" => Some("spike"),
        "checkpoint" => Some("execution"),
        "spike" => Some("spike"),
        _ => None,
    }
}

pub(super) fn has_column(
    conn: &Connection,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One-shot, durable migration of a pre-AGS-13 database. If the
/// `tickets` table still carries `ticket_type`, every row is checked
/// against the pinned mapping first — one unmappable value refuses
/// the open, naming the rows, with nothing changed — then, in a single
/// transaction, the column is renamed to `kind` and each row's value
/// is rewritten. Committed before the cache loads, so a restart (or a
/// SIGKILL right after) finds kinds in the file, never the old
/// values, and the check above is a no-op from then on. Returns how
/// many rows were rewritten (0: nothing to migrate).
pub(super) fn migrate_legacy_ticket_type(conn: &mut Connection) -> anyhow::Result<usize> {
    if !has_column(conn, "tickets", LEGACY_COLUMN)? {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare(&format!("SELECT id, {LEGACY_COLUMN} FROM tickets"))?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let unmappable: Vec<String> = rows
        .iter()
        .filter(|(_, tt)| legacy_kind(tt).is_none())
        .map(|(id, tt)| format!("{id} ({LEGACY_COLUMN} '{tt}')"))
        .collect();
    if !unmappable.is_empty() {
        anyhow::bail!(
            "refusing to open: {} ticket(s) carry a legacy {LEGACY_COLUMN} outside the pinned \
                 kind mapping (epic->scope, implementation->build, analysis->spike, \
                 checkpoint->execution, spike->spike): {}. Nothing was changed; fix those rows \
                 and reopen.",
            unmappable.len(),
            unmappable.join(", ")
        );
    }
    tx.execute(
        &format!("ALTER TABLE tickets RENAME COLUMN {LEGACY_COLUMN} TO kind"),
        [],
    )?;
    for (id, tt) in &rows {
        let kind = legacy_kind(tt).expect("checked above");
        tx.execute("UPDATE tickets SET kind = ?1 WHERE id = ?2", params![kind, id])?;
    }
    tx.commit()?;
    tracing::info!(
        "migrated {} tickets: {LEGACY_COLUMN} -> kind (AGS-13), durably",
        rows.len()
    );
    Ok(rows.len())
}

/// AGS-19: the `parent:` line of a body's YAML frontmatter, if the
/// body opens with a frontmatter block and carries one. The body is
/// the raw record (AGS-2 decides canonicality); this reads it, never
/// writes it.
pub(super) fn frontmatter_parent(body: &str) -> Option<String> {
    let mut lines = body.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("parent:") {
            let value = value.trim().trim_matches('"').trim();
            if value.is_empty() || value == "[]" || value == "null" {
                return None;
            }
            return Some(value.to_ascii_uppercase());
        }
    }
    None
}

/// Ticket ids in (epic, seq) order — the order the backfill considers
/// them in, so of two frontmatter parents that would close a loop the
/// earlier ticket's is the one kept. An id that is not `EPIC-N` sorts
/// last, by name.
fn by_epic_seq(id: &str) -> (String, u32, String) {
    match id.rsplit_once('-').and_then(|(e, n)| {
        n.parse::<u32>().ok().map(|n| (e.to_string(), n))
    }) {
        Some((epic, seq)) => (epic, seq, id.to_string()),
        None => (id.to_string(), u32::MAX, id.to_string()),
    }
}

/// True if making `parent` the parent of `child` would close a loop
/// in `edges` (child -> parent), `child == parent` included.
fn parent_edge_loops(edges: &HashMap<String, String>, child: &str, parent: &str) -> bool {
    let mut current = parent;
    for _ in 0..=edges.len() {
        if current == child {
            return true;
        }
        match edges.get(current) {
            Some(next) => current = next,
            None => return false,
        }
    }
    true
}

/// One-shot, durable migration of a pre-AGS-19 database (AGS-19).
///
/// If the `tickets` table has no `parent` column, this adds `parent`
/// and backfills the typed parent from each body's frontmatter
/// `parent: <id>`, in one transaction: an id that names an existing
/// ticket is written through, logged per ticket; one that resolves to
/// nothing — or that would make a ticket its own ancestor — is
/// logged and left empty. Never fatal: an unresolvable reference is a
/// dangling note in prose, not a corrupt store, and a later create
/// can still establish the relationship. (Until SCALE-4 it added a
/// `children` column too, and mirrored each edge there; the cache
/// derives children from `parent` now, so a database this migrates
/// never has one.)
///
/// Committed before the cache loads, so a restart (or a SIGKILL right
/// after) finds the typed parents in the file; the column check above
/// makes every later open a no-op, which also keeps startup off the
/// bodies of a million synthetic rows. Returns how many parents were
/// backfilled (0: nothing to migrate).
pub(super) fn migrate_add_parenthood(conn: &mut Connection) -> anyhow::Result<usize> {
    if has_column(conn, "tickets", "parent")? {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    tx.execute("ALTER TABLE tickets ADD COLUMN parent TEXT", [])?;

    let rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT id, body FROM tickets")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let known: HashSet<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    let mut proposed: Vec<(String, String)> = rows
        .iter()
        .filter_map(|(id, body)| frontmatter_parent(body).map(|p| (id.clone(), p)))
        .collect();
    proposed.sort_by_key(|(id, _)| by_epic_seq(id));

    let mut edges: HashMap<String, String> = HashMap::new();
    for (id, parent) in proposed {
        if !known.contains(parent.as_str()) {
            tracing::warn!(
                "AGS-19 migration: {id} names parent '{parent}' in its frontmatter, \
                     which is not a ticket — left empty"
            );
            continue;
        }
        if parent_edge_loops(&edges, &id, &parent) {
            tracing::warn!(
                "AGS-19 migration: {id} -> parent '{parent}' would make a ticket its own \
                     ancestor — left empty"
            );
            continue;
        }
        tracing::info!("AGS-19 migration: {id}.parent = {parent} (from body frontmatter)");
        edges.insert(id, parent);
    }

    for (child, parent) in &edges {
        tx.execute(
            "UPDATE tickets SET parent = ?1 WHERE id = ?2",
            params![parent, child],
        )?;
    }
    tx.commit()?;
    tracing::info!(
        "AGS-19: added parent to {} tickets, backfilling {} typed parent(s) \
             from body frontmatter, durably",
        rows.len(),
        edges.len()
    );
    Ok(edges.len())
}

/// The column SCALE-4 retires: AGS-19's `children`, a JSON list of every
/// child id on the parent's row. It exists in a database to be read once,
/// checked and dropped, never written.
const CHILDREN_COLUMN: &str = "children";

/// What [`migrate_drop_children`] found, for its caller and its log.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct DroppedChildren {
    /// Rows whose list named at least one child.
    pub lists: usize,
    /// Child ids across those lists.
    pub entries: usize,
    /// Where a list and the `parent` column disagreed, each one logged:
    /// a listed child whose `parent` is another ticket (or none), or a
    /// ticket naming a parent whose list left it out. `parent` stands.
    pub disagreements: usize,
}

/// One-shot, durable migration of a database written between AGS-19 and
/// SCALE-4 (SCALE-4).
///
/// If the `tickets` table still has `children`, every non-empty list is
/// read and checked against the `parent` column — the edge both ends
/// described, of which `children` was the server-kept mirror — and then,
/// in the same transaction, the column is dropped (`ALTER TABLE … DROP
/// COLUMN`, which rewrites each row without it). So the row no longer
/// holds a list, and no row can grow with its children again; the
/// children are what the cache derives from `parent`, in the store's
/// order. Every disagreement (none the server makes: `create` and
/// `set_parent` moved both ends in one transaction) is logged per id, and
/// `parent` stands — never fatal, as AGS-19's backfill is not. Committed
/// before the cache loads, so a restart (or a SIGKILL right after) finds
/// the column gone, and the column check makes every later open a no-op.
pub(super) fn migrate_drop_children(conn: &mut Connection) -> anyhow::Result<DroppedChildren> {
    if !has_column(conn, "tickets", CHILDREN_COLUMN)? {
        return Ok(DroppedChildren::default());
    }
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;
    let mut found = DroppedChildren::default();
    {
        // Every ticket's parent — the side that stands — and each list.
        let mut parent_of: HashMap<String, Option<String>> = HashMap::new();
        let mut listed: Vec<(String, Vec<String>)> = Vec::new();
        let mut stmt = tx.prepare(&format!("SELECT id, parent, {CHILDREN_COLUMN} FROM tickets"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            parent_of.insert(id.clone(), row.get(1)?);
            let raw: Option<String> = row.get(2)?;
            let kids = parse_ids(2, raw.as_deref().unwrap_or("[]"))?;
            if !kids.is_empty() {
                listed.push((id, kids));
            }
        }
        let mut named: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (child, parent) in &parent_of {
            if let Some(p) = parent {
                named.entry(p.as_str()).or_default().insert(child.as_str());
            }
        }
        let empty = HashSet::new();
        for (parent, kids) in &listed {
            found.lists += 1;
            found.entries += kids.len();
            let named_here = named.get(parent.as_str()).unwrap_or(&empty);
            for kid in kids {
                if !named_here.contains(kid.as_str()) {
                    found.disagreements += 1;
                    let actual = parent_of.get(kid).map(|p| p.as_deref().unwrap_or("no parent"));
                    tracing::warn!(
                        "SCALE-4 migration: {parent}'s children list named {kid}, whose parent is {} \
                         — the parent column stands",
                        actual.unwrap_or("not a ticket")
                    );
                }
            }
            let listed_here: HashSet<&str> = kids.iter().map(String::as_str).collect();
            for kid in named_here.iter().filter(|k| !listed_here.contains(**k)) {
                found.disagreements += 1;
                tracing::warn!(
                    "SCALE-4 migration: {kid} names {parent} as its parent, which its children \
                     list left out — counted as its child"
                );
            }
        }
        let had_list: HashSet<&str> = listed.iter().map(|(p, _)| p.as_str()).collect();
        for (parent, named_here) in &named {
            // A parent that is no ticket (a hand edit) had no row to list them.
            if !had_list.contains(parent) && parent_of.contains_key(*parent) {
                found.disagreements += named_here.len();
                tracing::warn!(
                    "SCALE-4 migration: {} ticket(s) name {parent} as their parent, whose children \
                     list was empty — counted as its children",
                    named_here.len()
                );
            }
        }
    }
    tx.execute(&format!("ALTER TABLE tickets DROP COLUMN {CHILDREN_COLUMN}"), [])?;
    tx.commit()?;
    tracing::info!(
        "SCALE-4: dropped the {CHILDREN_COLUMN} column ({} list(s), {} child id(s), {} \
         disagreement(s) with parent) in {:.1} ms, durably — children are derived from parent",
        found.lists,
        found.entries,
        found.disagreements,
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(found)
}

/// The column SDLC-9 adds: a ticket's linked commits, a JSON list like
/// `blocked_by` and `unlocks`.
const COMMITS_COLUMN: &str = "commits";

/// One-shot, durable migration of a database written before SDLC-9.
///
/// If the `tickets` table has no `commits` column, this adds it —
/// `TEXT NOT NULL DEFAULT '[]'`, the column [`SCHEMA`] creates, so a
/// migrated file and a new one have the same columns in the same order —
/// in one transaction, committed before the cache loads. Every existing
/// ticket then reads `[]`: no commit was ever linked to it. SQLite records
/// a constant default in the schema and rewrites no row to add the column,
/// so this costs the same at 10⁶ tickets as at ten. A restart (or a
/// SIGKILL right after) finds the column, and the check above makes every
/// later open a no-op. Additive: a binary from before SDLC-9 names its
/// columns, never `*`, so it still opens a migrated file — its inserts take
/// the default — and only does not see the commits. Returns whether the
/// column was added.
pub(super) fn migrate_add_commits(conn: &mut Connection) -> anyhow::Result<bool> {
    if has_column(conn, "tickets", COMMITS_COLUMN)? {
        return Ok(false);
    }
    let tx = conn.transaction()?;
    tx.execute(
        &format!("ALTER TABLE tickets ADD COLUMN {COMMITS_COLUMN} TEXT NOT NULL DEFAULT '[]'"),
        [],
    )?;
    let tickets: u64 = tx.query_row("SELECT count(*) FROM tickets", [], |r| r.get(0))?;
    tx.commit()?;
    tracing::info!(
        "SDLC-9: added the {COMMITS_COLUMN} column to {tickets} ticket(s), each with none linked, durably"
    );
    Ok(true)
}

/// The column WAVE-10 adds: a ticket's path address inside its shortcode.
const PATH_COLUMN: &str = "path";

/// One-shot, durable migration of a database written before WAVE-10.
///
/// If the `tickets` table has no `path` column, this adds it — `TEXT`,
/// nullable, no default, as [`SCHEMA`] creates it — in one transaction,
/// committed before the cache loads; every existing ticket then reads
/// `None`: no ticket had an address. SQLite rewrites no row to add a
/// column whose default is `NULL`, so this costs the same at 10⁶ tickets as
/// at ten, and no row is rewritten (WAVE-10: prod's data untouched). The
/// unique index on `(epic, path)` is made in the same transaction — and,
/// `IF NOT EXISTS`, on every open, which is a no-op once it is there. A
/// restart (or a SIGKILL right after) finds the column, and the check makes
/// every later open add nothing. Additive: a binary from before WAVE-10
/// names its columns, so it still opens a migrated file and inserts take
/// `NULL`. Returns whether the column was added.
pub(super) fn migrate_add_path(conn: &mut Connection) -> anyhow::Result<bool> {
    let tx = conn.transaction()?;
    let added = !has_column(&tx, "tickets", PATH_COLUMN)?;
    if added {
        tx.execute(&format!("ALTER TABLE tickets ADD COLUMN {PATH_COLUMN} TEXT"), [])?;
    }
    tx.execute(PATH_INDEX, [])?;
    tx.commit()?;
    if added {
        let tickets: u64 = conn.query_row("SELECT count(*) FROM tickets", [], |r| r.get(0))?;
        tracing::info!(
            "WAVE-10: added the {PATH_COLUMN} column to {tickets} ticket(s), none with a path, durably"
        );
    }
    Ok(added)
}

/// Serializes a unit enum to its serde string form (`Pending`,
/// `build`, `Critical`) — the same casing the wire uses, and what the
/// `FromStr` impls parse back.
pub(super) fn enum_str<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        other => unreachable!("unit enum must serialize to a string, got {other:?}"),
    }
}

fn parse_col<T: FromStr>(idx: usize, s: &str) -> rusqlite::Result<T>
where
    T::Err: std::fmt::Display,
{
    s.parse().map_err(|e: T::Err| {
        rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, format!("{e}").into())
    })
}

fn parse_ids(idx: usize, s: &str) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, format!("{e}").into())
    })
}

/// Inserts or replaces one ticket. A single implicit transaction:
/// when this returns Ok, the row is durable. SCALE-28: `child_count` with
/// it — a newborn's is 0 but where a hand edit already named it a parent.
pub(super) fn upsert(conn: &Connection, t: &Ticket) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tickets
                 (id, epic, seq, title, status, kind, severity,
                  blocked_by, unlocks, parent, body,
                  created_unix, updated_unix, commits, child_count, path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT (id) DO UPDATE SET
                 epic = excluded.epic,
                 seq = excluded.seq,
                 title = excluded.title,
                 status = excluded.status,
                 kind = excluded.kind,
                 severity = excluded.severity,
                 blocked_by = excluded.blocked_by,
                 unlocks = excluded.unlocks,
                 parent = excluded.parent,
                 body = excluded.body,
                 created_unix = excluded.created_unix,
                 updated_unix = excluded.updated_unix,
                 commits = excluded.commits,
                 child_count = excluded.child_count,
                 path = excluded.path",
        params![
            t.id,
            t.epic,
            t.seq,
            t.title,
            enum_str(&t.status),
            enum_str(&t.kind),
            t.severity.as_ref().map(enum_str),
            serde_json::to_string(&t.blocked_by).expect("serialize ids"),
            serde_json::to_string(&t.unlocks).expect("serialize ids"),
            t.parent,
            t.body,
            t.created_unix,
            t.updated_unix,
            serde_json::to_string(&t.commits).expect("serialize commits"),
            t.child_count,
            t.path,
        ],
    )?;
    Ok(())
}

/// PERF-2: inserts many fresh rows inside the caller's transaction
/// through one prepared statement. Plain INSERT (no upsert): seeded
/// ids are allocated above existing maxima, so a conflict here would
/// be a generator bug and must surface, not overwrite. SCALE-28: each
/// with the `child_count` it was generated with (0); the seed stores the
/// counts once the whole seed is in. SCALE-32: and each with its count of
/// unsatisfied blockers, `open` (one per ticket, in order) — worked out
/// before the rows are written, so no row is written twice.
pub(super) fn insert_many(conn: &Connection, tickets: &[Ticket], open: &[u32]) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO tickets
                 (id, epic, seq, title, status, kind, severity,
                  blocked_by, unlocks, parent, body,
                  created_unix, updated_unix, commits, child_count, unsatisfied, path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )?;
    debug_assert_eq!(tickets.len(), open.len());
    for (t, open) in tickets.iter().zip(open) {
        stmt.execute(params![
            t.id,
            t.epic,
            t.seq,
            t.title,
            enum_str(&t.status),
            enum_str(&t.kind),
            t.severity.as_ref().map(enum_str),
            serde_json::to_string(&t.blocked_by).expect("serialize ids"),
            serde_json::to_string(&t.unlocks).expect("serialize ids"),
            t.parent,
            t.body,
            t.created_unix,
            t.updated_unix,
            serde_json::to_string(&t.commits).expect("serialize commits"),
            t.child_count,
            open,
            t.path,
        ])?;
    }
    Ok(())
}

/// SCALE-14: every stored column of a row but the body, in the order
/// [`read_row`] reads them — what the cache loads, and the head of what a
/// whole ticket is read back as. SDLC-9: `commits` at 12. SCALE-28:
/// `child_count` at 13. WAVE-10: `path` last, at 14.
const ROW_COLUMNS: &str = "id, epic, seq, title, status, kind, severity,
                    blocked_by, unlocks, created_unix, updated_unix, parent, commits, child_count, path";

/// [`ROW_COLUMNS`] for a file that may be from before WAVE-10 — what
/// `--verify-derived` reads, which runs no migration: `NULL` stands in for
/// a `path` column the file does not have yet.
fn row_columns(conn: &Connection) -> rusqlite::Result<String> {
    Ok(if has_column(conn, "tickets", PATH_COLUMN)? {
        ROW_COLUMNS.to_string()
    } else {
        ROW_COLUMNS.replace(", path", ", NULL AS path")
    })
}

/// One row from a `SELECT` that starts with [`ROW_COLUMNS`].
fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        id: row.get(0)?,
        epic: row.get(1)?,
        seq: row.get(2)?,
        title: row.get(3)?,
        status: parse_col(4, &row.get::<_, String>(4)?)?,
        kind: parse_col(5, &row.get::<_, String>(5)?)?,
        severity: row
            .get::<_, Option<String>>(6)?
            .as_deref()
            .map(|s| parse_col(6, s))
            .transpose()?,
        blocked_by: parse_ids(7, &row.get::<_, String>(7)?)?,
        unlocks: parse_ids(8, &row.get::<_, String>(8)?)?,
        created_unix: row.get(9)?,
        updated_unix: row.get(10)?,
        parent: row.get::<_, Option<String>>(11)?,
        // SCALE-28: stored, moved by the transaction of every birth or move
        // under this ticket (SCALE-4 counted it from the parents at load).
        child_count: row.get(13)?,
        commits: parse_ids(12, &row.get::<_, String>(12)?)?,
        // WAVE-10: set at birth, never moved.
        path: row.get(14)?,
        // WAVE-6: not stored — the cache places every row at open.
        placement: None,
    })
}

/// SCALE-14: every ticket's row, and no body — what the cache holds. The
/// body is never read here, not read and dropped: what a load allocates
/// and frees still costs resident memory the allocator keeps.
pub(super) fn load_rows(conn: &Connection) -> anyhow::Result<HashMap<String, Row>> {
    let mut stmt = conn.prepare(&format!("SELECT {} FROM tickets", row_columns(conn)?))?;
    let rows = stmt.query_map([], read_row)?;
    let mut out = HashMap::new();
    for row in rows {
        let row = row?;
        out.insert(row.id.clone(), row);
    }
    Ok(out)
}

/// SCALE-32: [`load_rows`], and each ticket's count of unsatisfied blockers
/// that is not 0 — from the same statement, beside its row, so a start reads
/// the column in the pass that loads the rows and not in a second scan of
/// the table. Not on the row: the row is the wire's summary, and the count
/// is the store's own.
pub(super) fn load_rows_counted(
    conn: &Connection,
) -> anyhow::Result<(HashMap<String, Row>, HashMap<String, u32>)> {
    let mut stmt = conn.prepare(&format!("SELECT {ROW_COLUMNS}, unsatisfied FROM tickets"))?;
    let rows = stmt.query_map([], |r| Ok((read_row(r)?, r.get::<_, u32>(15)?)))?;
    let (mut out, mut open) = (HashMap::new(), HashMap::new());
    for row in rows {
        let (row, n) = row?;
        if n > 0 {
            open.insert(row.id.clone(), n);
        }
        out.insert(row.id.clone(), row);
    }
    Ok((out, open))
}

/// SCALE-14: one ticket's body, or `None` when no row has `id`.
pub(super) fn body(conn: &Connection, id: &str) -> rusqlite::Result<Option<String>> {
    conn.prepare_cached("SELECT body FROM tickets WHERE id = ?1")?
        .query_row([id], |row| row.get(0))
        .optional()
}

/// SCALE-14: one whole ticket as stored — its row and its body from one
/// statement, so from one snapshot (SCALE-28: its `child_count` too).
pub(super) fn ticket(conn: &Connection, id: &str) -> rusqlite::Result<Option<Ticket>> {
    conn.prepare_cached(&format!("SELECT {ROW_COLUMNS}, body FROM tickets WHERE id = ?1"))?
        .query_row([id], |row| Ok(super::whole(read_row(row)?, row.get(15)?)))
        .optional()
}

/// SCALE-14: rewrites every stored column of an existing ticket but its
/// body, which the cache does not hold and the write did not change — the
/// shape of every write after birth but `amend`'s. One row or an error:
/// the ticket is in the cache, so it is in the database. SDLC-9: the
/// commits are a column of the row, so `link_commits` writes through here.
/// SCALE-28: and not `child_count` — the derived state's, written only by
/// the birth or move that changes it ([`super::stored::Changes`]). WAVE-10:
/// nor `path`, which is set at birth and that no write moves.
pub(super) fn update_row(conn: &Connection, t: &Row) -> rusqlite::Result<()> {
    let changed = conn.prepare_cached(
        "UPDATE tickets SET
                 epic = ?2, seq = ?3, title = ?4, status = ?5, kind = ?6,
                 severity = ?7, blocked_by = ?8, unlocks = ?9, parent = ?10,
                 created_unix = ?11, updated_unix = ?12, commits = ?13
             WHERE id = ?1",
    )?
    .execute(params![
        t.id,
        t.epic,
        t.seq,
        t.title,
        enum_str(&t.status),
        enum_str(&t.kind),
        t.severity.as_ref().map(enum_str),
        serde_json::to_string(&t.blocked_by).expect("serialize ids"),
        serde_json::to_string(&t.unlocks).expect("serialize ids"),
        t.parent,
        t.created_unix,
        t.updated_unix,
        serde_json::to_string(&t.commits).expect("serialize commits"),
    ])?;
    match changed {
        1 => Ok(()),
        n => Err(rusqlite::Error::StatementChangedRows(n)),
    }
}

/// SCALE-14: replaces one ticket's body — `amend`'s, the one write after
/// birth that changes it.
pub(super) fn set_body(conn: &Connection, id: &str, body: &str) -> rusqlite::Result<()> {
    match conn
        .prepare_cached("UPDATE tickets SET body = ?2 WHERE id = ?1")?
        .execute(params![id, body])?
    {
        1 => Ok(()),
        n => Err(rusqlite::Error::StatementChangedRows(n)),
    }
}
