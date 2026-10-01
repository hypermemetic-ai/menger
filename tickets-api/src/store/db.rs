use super::Ticket;
use rusqlite::types::Type;
use rusqlite::{params, Connection};
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
            children     TEXT NOT NULL DEFAULT '[]',
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

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

/// Ticket ids in (epic, seq) order — the order `create` would have
/// appended them in, so a backfilled `children` list reads like a
/// grown one. An id that is not `EPIC-N` sorts last, by name.
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
/// and `children` and backfills the typed parent from each body's
/// frontmatter `parent: <id>`, in one transaction: an id that names
/// an existing ticket is written through (and mirrored in that
/// ticket's `children`), logged per ticket; one that resolves to
/// nothing — or that would make a ticket its own ancestor — is
/// logged and left empty. Never fatal: an unresolvable reference is a
/// dangling note in prose, not a corrupt store, and a later create
/// can still establish the relationship.
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
    tx.execute(
        "ALTER TABLE tickets ADD COLUMN children TEXT NOT NULL DEFAULT '[]'",
        [],
    )?;

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

    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for (child, parent) in &edges {
        children.entry(parent.as_str()).or_default().push(child);
    }
    for (child, parent) in &edges {
        tx.execute(
            "UPDATE tickets SET parent = ?1 WHERE id = ?2",
            params![parent, child],
        )?;
    }
    for (parent, mut kids) in children {
        kids.sort_by_key(|id| by_epic_seq(id));
        tx.execute(
            "UPDATE tickets SET children = ?1 WHERE id = ?2",
            params![
                serde_json::to_string(&kids).expect("serialize ids"),
                parent
            ],
        )?;
    }
    tx.commit()?;
    tracing::info!(
        "AGS-19: added parent/children to {} tickets, backfilling {} typed parent(s) \
             from body frontmatter, durably",
        rows.len(),
        edges.len()
    );
    Ok(edges.len())
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
/// when this returns Ok, the row is durable.
pub(super) fn upsert(conn: &Connection, t: &Ticket) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tickets
                 (id, epic, seq, title, status, kind, severity,
                  blocked_by, unlocks, parent, children, body,
                  created_unix, updated_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
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
                 children = excluded.children,
                 body = excluded.body,
                 created_unix = excluded.created_unix,
                 updated_unix = excluded.updated_unix",
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
            serde_json::to_string(&t.children).expect("serialize ids"),
            t.body,
            t.created_unix,
            t.updated_unix,
        ],
    )?;
    Ok(())
}

/// PERF-2: inserts many fresh rows inside the caller's transaction
/// through one prepared statement. Plain INSERT (no upsert): seeded
/// ids are allocated above existing maxima, so a conflict here would
/// be a generator bug and must surface, not overwrite.
pub(super) fn insert_many(conn: &Connection, tickets: &[Ticket]) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO tickets
                 (id, epic, seq, title, status, kind, severity,
                  blocked_by, unlocks, parent, children, body,
                  created_unix, updated_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    for t in tickets {
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
            serde_json::to_string(&t.children).expect("serialize ids"),
            t.body,
            t.created_unix,
            t.updated_unix,
        ])?;
    }
    Ok(())
}

pub(super) fn load_all(conn: &Connection) -> anyhow::Result<HashMap<String, Ticket>> {
    let mut stmt = conn.prepare(
        "SELECT id, epic, seq, title, status, kind, severity,
                    blocked_by, unlocks, body, created_unix, updated_unix,
                    parent, children
             FROM tickets",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Ticket {
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
            body: row.get(9)?,
            created_unix: row.get(10)?,
            updated_unix: row.get(11)?,
            parent: row.get::<_, Option<String>>(12)?,
            children: parse_ids(
                13,
                row.get::<_, Option<String>>(13)?.as_deref().unwrap_or("[]"),
            )?,
        })
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let ticket = row?;
        out.insert(ticket.id.clone(), ticket);
    }
    Ok(out)
}
