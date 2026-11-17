//! **What is kept** — the service's own file, `logs.db` (never the tickets
//! file):
//!
//! | table | what |
//! |---|---|
//! | `logs` | a log: its name, its ticket's uid, its schema as last defined or extended, its fields, who made it and when, its next number and its count, whether (and why) it is deprecated |
//! | `log_<name>` | a log's entries, one table a log (owner: "table per schema"): the envelope's columns `seq` (its key), `at`, `by`, `tags`, `ticket`, then a column a field |
//! | `entry_tags` | each entry's tags, `(log, tag, seq)`, so a tag is found in every log at once |
//!
//! **No SQL is built from an unchecked name.** A log's name and a field's
//! are identifiers by their types' rule ([`crate::vocab::identifier_ok`]: a
//! lower-case letter, then lower-case letters, digits and `_`), checked again
//! here before either is written into a statement, and always quoted; every
//! value is bound. An entry is kept only as its log's fields stand when it is
//! written: the check runs again inside the write's transaction, so an
//! `extend` or a `deprecate` between a call's check and its write is seen.

use crate::schema::{self, Field, Misfit};
use crate::vocab::identifier_ok;
use rusqlite::types::Value as Sql;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const TABLES: &str = "
CREATE TABLE IF NOT EXISTS logs (
    name TEXT PRIMARY KEY,
    uid TEXT NOT NULL,
    schema TEXT NOT NULL,
    fields TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    next_seq INTEGER NOT NULL,
    entries INTEGER NOT NULL,
    deprecated TEXT,
    deprecated_by TEXT,
    deprecated_ms INTEGER
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS logs_uid ON logs (uid);
CREATE TABLE IF NOT EXISTS entry_tags (
    log TEXT NOT NULL,
    tag TEXT NOT NULL,
    seq INTEGER NOT NULL,
    PRIMARY KEY (log, tag, seq)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS entry_tags_by_tag ON entry_tags (tag, log, seq);
";

/// Why the store did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A log of that name exists.
    Taken(String),
    /// No log of that name.
    NoLog(String),
    /// No field of that name in the log.
    NoField { log: String, field: String },
    /// The log, or one of its fields, is deprecated: why.
    Deprecated { log: String, field: Option<String>, message: String },
    /// The entry at `index` does not fit the log's fields.
    Misfit { index: usize, misfit: Misfit },
    /// A schema that does not grow the log's: the field, and why.
    NotGrown(Misfit),
    /// A name that is no identifier reached the store (never, past the types).
    BadName(String),
    /// The file failed, in words.
    Db(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Taken(n) => write!(f, "the log `{n}` exists: a log's name is its own"),
            StoreError::NoLog(n) => write!(f, "no log `{n}`"),
            StoreError::NoField { log, field } => write!(f, "the log `{log}` has no field `{field}`"),
            StoreError::Deprecated { log, field: None, message } => write!(f, "the log `{log}` is deprecated: {message} (it takes no new entry; what it holds stays readable)"),
            StoreError::Deprecated { log, field: Some(fd), message } => write!(f, "`{fd}` of the log `{log}` is deprecated: {message}"),
            StoreError::Misfit { index, misfit } => write!(f, "entry {index}: {}", misfit.why),
            StoreError::NotGrown(m) => write!(f, "{}", m.why),
            StoreError::BadName(n) => write!(f, "`{n}` is no name the store writes"),
            StoreError::Db(e) => write!(f, "the logs store: {e}"),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e.to_string())
    }
}

/// A log as `logs` keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRow {
    pub name: String,
    pub uid: String,
    pub schema: String,
    pub fields: Vec<Field>,
    pub created_by: String,
    pub created_ms: i64,
    pub next_seq: u64,
    pub entries: u64,
    pub deprecated: Option<String>,
}

/// One kept entry: its envelope, and its fields' values as the entry carried them.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryRow {
    pub seq: u64,
    pub at: i64,
    pub by: String,
    pub tags: Vec<String>,
    pub ticket: Option<String>,
    pub entry: serde_json::Map<String, Value>,
}

/// What [`LogsStore::append`] kept: the first and last numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    pub first: u64,
    pub last: u64,
}

/// `"name"`, once `name` is an identifier — the one way a name enters SQL.
pub(crate) fn quoted(name: &str) -> Result<String, StoreError> {
    match identifier_ok(name) {
        true => Ok(format!("\"{name}\"")),
        false => Err(StoreError::BadName(name.to_string())),
    }
}

/// The table of the log `name`, quoted.
pub(crate) fn table(name: &str) -> Result<String, StoreError> {
    quoted(name).map(|_| format!("\"log_{name}\""))
}

/// The store over `logs.db`.
#[derive(Debug)]
pub struct LogsStore {
    path: PathBuf,
    conn: Mutex<Connection>,
}

fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<LogRow> {
    let fields: String = r.get(3)?;
    Ok(LogRow {
        name: r.get(0)?,
        uid: r.get(1)?,
        schema: r.get(2)?,
        fields: serde_json::from_str(&fields).map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?,
        created_by: r.get(4)?,
        created_ms: r.get(5)?,
        next_seq: r.get::<_, i64>(6)? as u64,
        entries: r.get::<_, i64>(7)? as u64,
        deprecated: r.get(8)?,
    })
}

const LOG_COLUMNS: &str = "name, uid, schema, fields, created_by, created_ms, next_seq, entries, deprecated";

fn log_in(tx: &Connection, name: &str) -> Result<LogRow, StoreError> {
    tx.query_row(&format!("SELECT {LOG_COLUMNS} FROM logs WHERE name = ?1"), [name], row_of).optional()?.ok_or_else(|| StoreError::NoLog(name.into()))
}

fn fields_text(fields: &[Field]) -> Result<String, StoreError> {
    serde_json::to_string(fields).map_err(|e| StoreError::Db(e.to_string()))
}

impl LogsStore {
    /// The store at `path`, made if missing (its directory too).
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Db(format!("{}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(TABLES)?;
        Ok(Self { path: path.to_path_buf(), conn: Mutex::new(conn) })
    }

    /// Where it is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The log `name`.
    pub fn log(&self, name: &str) -> Result<Option<LogRow>, StoreError> {
        Ok(self.conn().query_row(&format!("SELECT {LOG_COLUMNS} FROM logs WHERE name = ?1"), [name], row_of).optional()?)
    }

    /// Every log, by name.
    pub fn logs(&self) -> Result<Vec<LogRow>, StoreError> {
        let conn = self.conn();
        let mut st = conn.prepare(&format!("SELECT {LOG_COLUMNS} FROM logs ORDER BY name"))?;
        let rows = st.query_map([], row_of)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// How many logs there are.
    pub fn count(&self) -> u64 {
        self.conn().query_row("SELECT COUNT(*) FROM logs", [], |r| r.get::<_, i64>(0)).map_or(0, |n| n as u64)
    }

    /// The log `name` made from `fields` (its schema `schema`), tied to the
    /// ticket `uid`, its table with it — in one transaction.
    pub fn define(&self, name: &str, uid: &str, schema: &str, fields: &[Field], by: &str, now_ms: i64) -> Result<(), StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if tx.query_row("SELECT 1 FROM logs WHERE name = ?1", [name], |_| Ok(())).optional()?.is_some() {
            return Err(StoreError::Taken(name.into()));
        }
        let t = table(name)?;
        let mut cols = vec!["seq INTEGER PRIMARY KEY".to_string(), "at INTEGER NOT NULL".into(), "by TEXT NOT NULL".into(), "tags TEXT NOT NULL".into(), "ticket TEXT".into()];
        for f in fields {
            cols.push(format!("{} {}", quoted(&f.name)?, f.kind.sql()));
        }
        tx.execute_batch(&format!("CREATE TABLE {t} ({}); CREATE INDEX \"log_{name}_at\" ON {t} (at);", cols.join(", ")))?;
        tx.execute(
            &format!("INSERT INTO logs ({LOG_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 0, NULL)"),
            params![name, uid, schema, fields_text(fields)?, by, now_ms],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// `entries` appended to the log `name` by `by`, each with `tags` and
    /// `ticket` — checked against the log's fields as they stand inside the
    /// transaction; all kept or none.
    pub fn append(&self, name: &str, entries: &[Value], tags: &[String], ticket: Option<&str>, by: &str, now_ms: i64) -> Result<Kept, StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let log = log_in(&tx, name)?;
        let rows = writable(&log, entries)?;
        let first = log.next_seq;
        insert(&tx, &log, &rows, tags, ticket, by, now_ms)?;
        let n = rows.len() as u64;
        tx.execute("UPDATE logs SET next_seq = next_seq + ?2, entries = entries + ?2 WHERE name = ?1", params![name, n as i64])?;
        tx.commit()?;
        Ok(Kept { first, last: first + n - 1 })
    }

    /// The log `name`'s schema grown to `next` (its text `schema`) by
    /// [`schema::grow`] against its fields as they stand inside the
    /// transaction, each field added a new column; the fields after, and
    /// those added.
    pub fn extend(&self, name: &str, schema: &str, next: Vec<Field>) -> Result<(Vec<Field>, Vec<Field>), StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let log = log_in(&tx, name)?;
        let (now, added) = schema::grow(&log.fields, next).map_err(StoreError::NotGrown)?;
        let t = table(name)?;
        for f in &added {
            tx.execute_batch(&format!("ALTER TABLE {t} ADD COLUMN {} {}", quoted(&f.name)?, f.kind.sql()))?;
        }
        tx.execute("UPDATE logs SET schema = ?2, fields = ?3 WHERE name = ?1", params![name, schema, fields_text(&now)?])?;
        tx.commit()?;
        Ok((now, added))
    }

    /// The log `name` (or its `field`) closed to new entries, for `message`.
    /// One deprecated already says so, and is left as it was.
    pub fn deprecate(&self, name: &str, field: Option<&str>, message: &str, by: &str, now_ms: i64) -> Result<(), StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut log = log_in(&tx, name)?;
        match field {
            None => {
                if let Some(m) = log.deprecated {
                    return Err(StoreError::Deprecated { log: name.into(), field: None, message: m });
                }
                tx.execute("UPDATE logs SET deprecated = ?2, deprecated_by = ?3, deprecated_ms = ?4 WHERE name = ?1", params![name, message, by, now_ms])?;
            }
            Some(fd) => {
                let f = log.fields.iter_mut().find(|f| f.name == fd).ok_or_else(|| StoreError::NoField { log: name.into(), field: fd.into() })?;
                if let Some(m) = &f.deprecated {
                    return Err(StoreError::Deprecated { log: name.into(), field: Some(fd.into()), message: m.clone() });
                }
                f.deprecated = Some(message.to_string());
                f.required = false;
                tx.execute("UPDATE logs SET fields = ?2 WHERE name = ?1", params![name, fields_text(&log.fields)?])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The log `name`'s entries, newest first, at most `limit`, before the
    /// number `before` when given — every one as it was kept.
    pub fn entries(&self, name: &str, before: Option<u64>, limit: usize) -> Result<Vec<EntryRow>, StoreError> {
        let conn = self.conn();
        let log = log_in(&conn, name)?;
        let t = table(name)?;
        let cols: Vec<String> = log.fields.iter().map(|f| quoted(&f.name)).collect::<Result<_, _>>()?;
        let sql = format!(
            "SELECT seq, at, by, tags, ticket{}{} FROM {t} WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2",
            if cols.is_empty() { "" } else { ", " },
            cols.join(", ")
        );
        let mut st = conn.prepare(&sql)?;
        let before = before.map_or(i64::MAX, |b| b as i64);
        let rows = st.query_map(params![before, limit as i64], |r| entry_of(r, &log.fields))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// Where a read stands: before which entry it goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Before {
    /// From the newest.
    Start,
    /// One log's: entries numbered below this.
    Seq(u64),
    /// Across logs, ordered by `at` (newest first), then the log's name,
    /// then `seq` (highest first): entries after this one.
    At { at: i64, log: String, seq: u64 },
}

/// LOGS-3: what a read asks of one log's table — each condition bound,
/// the filter compiled against the log's columns (or the envelope's).
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub by: Option<String>,
    pub ticket: Option<String>,
    pub tag: Option<String>,
    pub filter: Option<crate::filter::Expr>,
    /// Whose columns the filter is checked against, in words.
    pub whose: String,
    /// The envelope's alone (a read across logs).
    pub envelope_only: bool,
    pub limit: usize,
}

impl LogsStore {
    /// LOGS-3: `log`'s entries that `q` asks for, `before` where the read
    /// stands — one log's newest first by `seq`, or (across logs, `Before::At`
    /// or `envelope_only`) by `at`, then `seq`. `Err(why)` in words for a
    /// filter its columns refuse; a store failure as [`StoreError`].
    pub fn read(&self, log: &LogRow, q: &Query, before: &Before) -> Result<Result<Vec<EntryRow>, String>, StoreError> {
        let t = table(&log.name)?;
        let cols: Vec<String> = log.fields.iter().map(|f| quoted(&f.name)).collect::<Result<_, _>>()?;
        let mut conds: Vec<String> = Vec::new();
        let mut values: Vec<Sql> = Vec::new();
        let bind = |sql: &str, v: Sql, values: &mut Vec<Sql>| {
            values.push(v);
            sql.replace('?', &format!("?{}", values.len()))
        };
        match before {
            Before::Start => {}
            Before::Seq(seq) => conds.push(bind("seq < ?", Sql::Integer(*seq as i64), &mut values)),
            Before::At { at, log: name, seq } => {
                // Ordered by at (newest first), the log's name, then seq
                // (highest first): what comes after (at, name, seq).
                values.push(Sql::Integer(*at));
                let a = values.len();
                conds.push(match log.name.as_str().cmp(name.as_str()) {
                    std::cmp::Ordering::Less => format!("at < ?{a}"),
                    std::cmp::Ordering::Greater => format!("at <= ?{a}"),
                    std::cmp::Ordering::Equal => {
                        values.push(Sql::Integer(*seq as i64));
                        format!("(at < ?{a} OR (at = ?{a} AND seq < ?{}))", values.len())
                    }
                });
            }
        }
        if let Some(v) = q.since {
            conds.push(bind("at >= ?", Sql::Integer(v), &mut values));
        }
        if let Some(v) = q.until {
            conds.push(bind("at < ?", Sql::Integer(v), &mut values));
        }
        if let Some(v) = &q.by {
            conds.push(bind("by = ?", Sql::Text(v.clone()), &mut values));
        }
        if let Some(v) = &q.ticket {
            conds.push(bind("ticket = ?", Sql::Text(v.clone()), &mut values));
        }
        if let Some(tag) = &q.tag {
            let l = bind("log = ?", Sql::Text(log.name.clone()), &mut values);
            let g = bind("tag = ?", Sql::Text(tag.clone()), &mut values);
            conds.push(format!("seq IN (SELECT seq FROM entry_tags WHERE {l} AND {g})"));
        }
        if let Some(e) = &q.filter {
            let named = if q.envelope_only { crate::filter::envelope() } else { crate::filter::of_log(&log.fields) };
            match crate::filter::compile(e, &named, &q.whose, values.len() + 1) {
                Ok(c) => {
                    conds.push(c.sql);
                    values.extend(c.values);
                }
                Err(why) => return Ok(Err(why)),
            }
        }
        let order = if q.envelope_only || matches!(before, Before::At { .. }) { "at DESC, seq DESC" } else { "seq DESC" };
        let limit = bind("?", Sql::Integer(q.limit as i64), &mut values);
        let sql = format!(
            "SELECT seq, at, by, tags, ticket{}{} FROM {t}{}{} ORDER BY {order} LIMIT {limit}",
            if cols.is_empty() { "" } else { ", " },
            cols.join(", "),
            if conds.is_empty() { "" } else { " WHERE " },
            conds.join(" AND "),
        );
        let conn = self.conn();
        let mut st = conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(values.iter()), |r| entry_of(r, &log.fields))?.collect::<Result<Vec<_>, _>>()?;
        Ok(Ok(rows))
    }

    /// The entries of the log `name` numbered `first` to `last` — what an
    /// append just kept, as a watcher is told it.
    pub fn kept(&self, name: &str, first: u64, last: u64) -> Result<Vec<EntryRow>, StoreError> {
        let conn = self.conn();
        let log = log_in(&conn, name)?;
        let t = table(name)?;
        let cols: Vec<String> = log.fields.iter().map(|f| quoted(&f.name)).collect::<Result<_, _>>()?;
        let sql = format!("SELECT seq, at, by, tags, ticket{}{} FROM {t} WHERE seq >= ?1 AND seq <= ?2 ORDER BY seq", if cols.is_empty() { "" } else { ", " }, cols.join(", "));
        let mut st = conn.prepare(&sql)?;
        let rows = st.query_map(params![first as i64, last as i64], |r| entry_of(r, &log.fields))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// A kept row as an entry: the envelope (columns 0–4), then the fields.
pub(crate) fn entry_of(r: &rusqlite::Row<'_>, fields: &[Field]) -> rusqlite::Result<EntryRow> {
    let tags: String = r.get(3)?;
    let mut entry = serde_json::Map::new();
    for (i, f) in fields.iter().enumerate() {
        let v = schema::json_of(f.kind, r.get::<_, Sql>(5 + i)?);
        if !v.is_null() {
            entry.insert(f.name.clone(), v);
        }
    }
    Ok(EntryRow { seq: r.get::<_, i64>(0)? as u64, at: r.get(1)?, by: r.get(2)?, tags: serde_json::from_str(&tags).unwrap_or_default(), ticket: r.get(4)?, entry })
}

/// `entries` as rows of `log`, or why not: the log deprecated, an entry
/// that does not fit (by its index).
pub fn writable(log: &LogRow, entries: &[Value]) -> Result<Vec<Vec<Sql>>, StoreError> {
    if let Some(m) = &log.deprecated {
        return Err(StoreError::Deprecated { log: log.name.clone(), field: None, message: m.clone() });
    }
    entries.iter().enumerate().map(|(index, e)| schema::check(&log.fields, e).map_err(|misfit| StoreError::Misfit { index, misfit })).collect()
}

fn insert(tx: &Transaction<'_>, log: &LogRow, rows: &[Vec<Sql>], tags: &[String], ticket: Option<&str>, by: &str, now_ms: i64) -> Result<(), StoreError> {
    let t = table(&log.name)?;
    let mut names = vec!["seq".to_string(), "at".into(), "by".into(), "tags".into(), "ticket".into()];
    for f in &log.fields {
        names.push(quoted(&f.name)?);
    }
    let marks: Vec<String> = (1..=names.len()).map(|i| format!("?{i}")).collect();
    let mut st = tx.prepare(&format!("INSERT INTO {t} ({}) VALUES ({})", names.join(", "), marks.join(", ")))?;
    let mut tag_st = tx.prepare("INSERT INTO entry_tags (log, tag, seq) VALUES (?1, ?2, ?3)")?;
    let tags_text = serde_json::to_string(tags).map_err(|e| StoreError::Db(e.to_string()))?;
    for (i, row) in rows.iter().enumerate() {
        let seq = (log.next_seq + i as u64) as i64;
        let envelope = [Sql::Integer(seq), Sql::Integer(now_ms), Sql::Text(by.into()), Sql::Text(tags_text.clone()), ticket.map_or(Sql::Null, |t| Sql::Text(t.into()))];
        st.execute(params_from_iter(envelope.iter().chain(row.iter())))?;
        for tag in tags {
            tag_st.execute(params![log.name, tag, seq])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SCHEMA: &str = r#"{"type": "object", "required": ["app", "value"], "properties": {
        "app": {"type": "string"}, "value": {"type": "number"}, "machine": {"type": "string"},
        "ok": {"type": "boolean"}, "config": {"type": "object"}}}"#;

    fn store() -> (tempfile::TempDir, LogsStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = LogsStore::open(&dir.path().join("logs.db")).unwrap();
        (dir, s)
    }

    fn defined() -> (tempfile::TempDir, LogsStore) {
        let (d, s) = store();
        s.define("perf", "7k3mq2xa", SCHEMA, &schema::parse(SCHEMA).unwrap(), "key:me", 1_000).unwrap();
        (d, s)
    }

    #[test]
    fn a_log_is_a_table_and_its_name_is_its_own() {
        let (_d, s) = defined();
        let log = s.log("perf").unwrap().unwrap();
        assert_eq!((log.uid.as_str(), log.next_seq, log.entries, log.fields.len()), ("7k3mq2xa", 1, 0, 5));
        assert_eq!(s.define("perf", "x", SCHEMA, &log.fields, "key:me", 2), Err(StoreError::Taken("perf".into())));
        let cols: Vec<String> = s.conn().prepare("SELECT name FROM pragma_table_info('log_perf')").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(cols, ["seq", "at", "by", "tags", "ticket", "app", "value", "machine", "ok", "config"]);
        assert_eq!(s.define("Bad\"name", "x", SCHEMA, &log.fields, "k", 1), Err(StoreError::BadName("Bad\"name".into())), "never into SQL");
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn entries_are_kept_whole_or_not_at_all() {
        let (_d, s) = defined();
        let good = json!({"app": "plexus", "value": 1.5, "ok": true, "config": {"w": 4}});
        let k = s.append("perf", &[good.clone(), json!({"app": "menger", "value": 2})], &["plexus".into(), "websocket".into()], Some("9qxd1gn8"), "key:me", 5_000).unwrap();
        assert_eq!(k, Kept { first: 1, last: 2 });
        let bad = s.append("perf", &[good.clone(), json!({"app": "x", "value": "fast"}), good.clone()], &[], None, "key:me", 6_000).unwrap_err();
        assert!(matches!(&bad, StoreError::Misfit { index: 1, misfit } if misfit.field.as_deref() == Some("value")), "{bad:?}");
        let rows = s.entries("perf", None, 10).unwrap();
        assert_eq!(rows.len(), 2, "nothing of the refused call kept");
        assert_eq!((rows[0].seq, rows[0].entry["value"].clone()), (2, json!(2.0)));
        assert_eq!(rows[1].entry, json!({"app": "plexus", "value": 1.5, "ok": true, "config": {"w": 4}}).as_object().unwrap().clone());
        assert_eq!((rows[1].at, rows[1].by.as_str(), rows[1].tags.clone(), rows[1].ticket.as_deref()), (5_000, "key:me", vec!["plexus".to_string(), "websocket".into()], Some("9qxd1gn8")));
        let tags: i64 = s.conn().query_row("SELECT COUNT(*) FROM entry_tags WHERE log = 'perf'", [], |r| r.get(0)).unwrap();
        assert_eq!(tags, 4);
        assert_eq!(s.log("perf").unwrap().unwrap().next_seq, 3);
        assert_eq!(s.entries("perf", Some(2), 10).unwrap().len(), 1, "a page before 2");
    }

    #[test]
    fn a_log_grows_and_old_entries_read_the_new_field_as_null() {
        let (_d, s) = defined();
        s.append("perf", &[json!({"app": "a", "value": 1})], &[], None, "k", 1).unwrap();
        let mut next: Value = serde_json::from_str(SCHEMA).unwrap();
        next["properties"]["p99"] = json!({"type": "number"});
        let text = next.to_string();
        let (all, added) = s.extend("perf", &text, schema::parse(&text).unwrap()).unwrap();
        assert_eq!((all.len(), added.len()), (6, 1));
        assert!(matches!(s.extend("perf", SCHEMA, schema::parse(SCHEMA).unwrap()), Err(StoreError::NotGrown(m)) if m.field.as_deref() == Some("p99")), "and never shrinks");
        s.append("perf", &[json!({"app": "a", "value": 1, "p99": 9.5})], &[], None, "k", 2).unwrap();
        let rows = s.entries("perf", None, 10).unwrap();
        assert_eq!(rows[0].entry.get("p99"), Some(&json!(9.5)));
        assert_eq!(rows[1].entry.get("p99"), None, "read as null: not carried");
        assert_eq!(s.log("perf").unwrap().unwrap().schema, text);
    }

    #[test]
    fn a_deprecated_log_or_field_takes_no_new_entry() {
        let (_d, s) = defined();
        s.append("perf", &[json!({"app": "a", "value": 1, "machine": "box"})], &[], None, "k", 1).unwrap();
        s.deprecate("perf", Some("machine"), "one box now", "k", 2).unwrap();
        let e = s.append("perf", &[json!({"app": "a", "value": 1, "machine": "box"})], &[], None, "k", 3).unwrap_err();
        assert!(e.to_string().contains("`machine` is deprecated: one box now"), "{e}");
        s.append("perf", &[json!({"app": "a", "value": 2})], &[], None, "k", 4).unwrap();
        assert_eq!(s.deprecate("perf", Some("nope"), "x", "k", 5), Err(StoreError::NoField { log: "perf".into(), field: "nope".into() }));
        s.deprecate("perf", None, "replaced by perf2", "k", 6).unwrap();
        let e = s.append("perf", &[json!({"app": "a", "value": 3})], &[], None, "k", 7).unwrap_err();
        assert_eq!(e, StoreError::Deprecated { log: "perf".into(), field: None, message: "replaced by perf2".into() });
        assert!(matches!(s.deprecate("perf", None, "again", "k", 8), Err(StoreError::Deprecated { .. })));
        assert_eq!(s.entries("perf", None, 10).unwrap().len(), 2, "what was written stays readable");
        assert_eq!(s.log("perf").unwrap().unwrap().deprecated.as_deref(), Some("replaced by perf2"));
    }

    #[test]
    fn the_store_reopens_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x/logs.db");
        {
            let s = LogsStore::open(&path).unwrap();
            s.define("perf", "u", SCHEMA, &schema::parse(SCHEMA).unwrap(), "k", 1).unwrap();
            s.append("perf", &[json!({"app": "a", "value": 1})], &["t".into()], None, "k", 1).unwrap();
        }
        let s = LogsStore::open(&path).unwrap();
        assert_eq!(s.entries("perf", None, 5).unwrap().len(), 1);
        assert_eq!(s.logs().unwrap().len(), 1);
    }
}
