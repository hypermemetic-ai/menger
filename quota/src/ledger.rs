//! **The ledger** — `quota.db`, the service's own file (never the tickets
//! file): what each principal has used per source (and per day, for a
//! windowed source), the reservations open against it, and the allowances —
//! a principal's, a team's, the default per source.
//!
//! **No double counting.** One connection behind a mutex, and every reserve
//! is one `IMMEDIATE` transaction that reads what is used and what is held
//! and writes the reservation, so two reserves near the cap never both pass:
//! the second sees the first's hold. A commit moves its hold into usage and
//! removes it in one transaction, so the amount is never in both or neither.
//! A reservation not committed by its expiry (an abandoned upload) is swept
//! — by every reserve before it reads, and by the service's sweep.
//!
//! | table | row |
//! |---|---|
//! | `usage` | (principal, source, window) → used |
//! | `holds` | a reservation: ticket → principal, source, window, amount, expiry |
//! | `allowances` | (kind: principal · team · default, who, source) → cap (`NULL`: unlimited) |
//! | `settings` | the plan the file started from: concurrency, the box lines, the hold time |

use crate::plan::{BoxLines, Cap, Plan};
use crate::sources::{self, Source};
use crate::Refusal;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const TABLES: &str = "
    CREATE TABLE IF NOT EXISTS usage (
        principal TEXT NOT NULL,
        source    TEXT NOT NULL,
        window    INTEGER NOT NULL,
        used      INTEGER NOT NULL,
        PRIMARY KEY (principal, source, window)
    );
    CREATE TABLE IF NOT EXISTS holds (
        ticket    TEXT PRIMARY KEY,
        principal TEXT NOT NULL,
        source    TEXT NOT NULL,
        window    INTEGER NOT NULL,
        amount    INTEGER NOT NULL,
        at        INTEGER NOT NULL,
        expires   INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS holds_by ON holds (principal, source);
    CREATE TABLE IF NOT EXISTS allowances (
        kind   TEXT NOT NULL,
        who    TEXT NOT NULL,
        source TEXT NOT NULL,
        cap    INTEGER,
        set_by TEXT NOT NULL,
        at     INTEGER NOT NULL,
        PRIMARY KEY (kind, who, source)
    );
    CREATE TABLE IF NOT EXISTS settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
";

/// Whose an allowance is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Holder {
    Principal(String),
    Team(String),
    Default,
}

impl Holder {
    fn columns(&self) -> (&'static str, &str) {
        match self {
            Holder::Principal(p) => ("principal", p),
            Holder::Team(t) => ("team", t),
            Holder::Default => ("default", ""),
        }
    }
}

/// Where a principal's effective cap came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapFrom {
    /// Their own allowance.
    Own,
    /// Their team's (the largest of their teams').
    Team(String),
    /// The default for the source.
    Default,
    /// An admin of `*` or the operator: never capped.
    Exempt,
}

impl std::fmt::Display for CapFrom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapFrom::Own => f.write_str("own"),
            CapFrom::Team(t) => write!(f, "team:{t}"),
            CapFrom::Default => f.write_str("default"),
            CapFrom::Exempt => f.write_str("exempt"),
        }
    }
}

/// Who is asking, as the ledger needs it: the principal, the teams it is in
/// (only those holding an allowance matter), and whether it is exempt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Who {
    pub principal: String,
    pub teams: Vec<String>,
    pub exempt: bool,
}

impl Who {
    pub fn plain(principal: &str) -> Self {
        Self { principal: principal.to_string(), teams: Vec::new(), exempt: false }
    }
}

/// A reservation made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reserved {
    pub ticket: String,
    /// What is left after it, under the cap; `None`: unlimited.
    pub remaining: Option<u64>,
    pub expires: i64,
}

/// A reservation committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    pub principal: String,
    pub source: String,
    pub amount: u64,
    /// Used, after it.
    pub used: u64,
}

/// One source of a principal's usage, as the surface shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub source: &'static Source,
    pub used: u64,
    /// Reserved, not yet committed.
    pub held: u64,
    pub cap: Cap,
    pub from: CapFrom,
}

/// Why the ledger did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    /// No open reservation of that ticket (committed, released, or swept).
    NoHold(String),
    /// A commit past what was reserved.
    PastHold { ticket: String, reserved: u64, amount: u64 },
    UnknownSource(String),
    Db(String),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::NoHold(t) => write!(f, "no open reservation `{t}` (committed, released, or expired and swept)"),
            LedgerError::PastHold { ticket, reserved, amount } => write!(f, "reservation `{ticket}` holds {reserved}; {amount} is more than that — reserve again for the rest"),
            LedgerError::UnknownSource(s) => write!(f, "`{s}` is no source (the sources: {})", sources::names()),
            LedgerError::Db(e) => write!(f, "the quota file: {e}"),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<rusqlite::Error> for LedgerError {
    fn from(e: rusqlite::Error) -> Self {
        LedgerError::Db(e.to_string())
    }
}

impl From<LedgerError> for Refusal {
    fn from(e: LedgerError) -> Self {
        match e {
            LedgerError::UnknownSource(source) => Refusal::UnknownSource { source },
            other => Refusal::Failed(other.to_string()),
        }
    }
}

/// The settings a file keeps from the plan it started from.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub plan: String,
    pub concurrent: BTreeMap<String, u32>,
    pub box_lines: BoxLines,
    pub hold_secs: i64,
}

/// The file, open.
pub struct Ledger {
    conn: Mutex<Connection>,
    path: PathBuf,
    settings: Settings,
}

impl std::fmt::Debug for Ledger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ledger").field("path", &self.path).field("plan", &self.settings.plan).finish()
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn known(source: &str) -> Result<&'static Source, LedgerError> {
    sources::source(source).ok_or_else(|| LedgerError::UnknownSource(source.to_string()))
}

impl Ledger {
    /// The ledger on `path`, made if missing. A fresh file takes `plan`'s
    /// defaults and settings, once; a file that has them keeps its own.
    pub fn open(path: &Path, plan: &Plan) -> Result<Self, LedgerError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| LedgerError::Db(format!("{}: {e}", dir.display())))?;
        }
        let mut conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;")?;
        conn.execute_batch(TABLES)?;
        let had: Option<String> = conn.query_row("SELECT value FROM settings WHERE key = 'plan'", [], |r| r.get(0)).optional()?;
        if had.is_none() {
            let tx = conn.transaction()?;
            let at = now();
            for (source, cap) in &plan.default {
                tx.execute(
                    "INSERT OR IGNORE INTO allowances (kind, who, source, cap, set_by, at) VALUES ('default', '', ?1, ?2, ?3, ?4)",
                    params![source, cap.stored(), format!("plan:{}", plan.name), at],
                )?;
            }
            let put = |k: &str, v: String| tx.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![k, v]).map(|_| ());
            put("plan", plan.name.clone())?;
            put("concurrent", serde_json::to_string(&plan.concurrent).expect("a map"))?;
            put("box.warn_percent", plan.box_lines.warn_percent.to_string())?;
            put("box.full_percent", plan.box_lines.full_percent.to_string())?;
            put("hold_secs", plan.hold_secs.to_string())?;
            tx.commit()?;
            tracing::info!("quota:     {} (new: the plan `{}`'s defaults)", path.display(), plan.name);
        }
        let get = |k: &str| -> Result<Option<String>, LedgerError> { Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| r.get(0)).optional()?) };
        let num = |k: &str, d: f64| -> Result<f64, LedgerError> { Ok(get(k)?.and_then(|v| v.parse().ok()).unwrap_or(d)) };
        let settings = Settings {
            plan: get("plan")?.unwrap_or_default(),
            concurrent: get("concurrent")?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default(),
            box_lines: BoxLines { warn_percent: num("box.warn_percent", 70.0)?, full_percent: num("box.full_percent", 85.0)? },
            hold_secs: num("hold_secs", 3600.0)? as i64,
        };
        Ok(Self { conn: Mutex::new(conn), path: path.to_path_buf(), settings })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `who`'s cap on `source` and where it came from — their own, else
    /// their teams' largest, else the default, else unlimited; exempt: none.
    fn cap_of(conn: &Connection, who: &Who, source: &str) -> rusqlite::Result<(Cap, CapFrom)> {
        if who.exempt {
            return Ok((Cap::Unlimited, CapFrom::Exempt));
        }
        let row = |kind: &str, w: &str| -> rusqlite::Result<Option<Cap>> {
            conn.prepare_cached("SELECT cap FROM allowances WHERE kind = ?1 AND who = ?2 AND source = ?3")?
                .query_row(params![kind, w, source], |r| r.get::<_, Option<i64>>(0))
                .optional()
                .map(|o| o.map(Cap::of_stored))
        };
        if let Some(c) = row("principal", &who.principal)? {
            return Ok((c, CapFrom::Own));
        }
        let mut best: Option<(Cap, String)> = None;
        for t in &who.teams {
            if let Some(c) = row("team", t)? {
                best = Some(match best {
                    Some((b, bt)) if b.max(c) == b => (b, bt),
                    _ => (c, t.clone()),
                });
            }
        }
        if let Some((c, t)) = best {
            return Ok((c, CapFrom::Team(t)));
        }
        Ok(match row("default", "")? {
            Some(c) => (c, CapFrom::Default),
            None => (Cap::Unlimited, CapFrom::Default),
        })
    }

    fn used(conn: &Connection, principal: &str, source: &str, window: i64) -> rusqlite::Result<u64> {
        conn.prepare_cached("SELECT used FROM usage WHERE principal = ?1 AND source = ?2 AND window = ?3")?
            .query_row(params![principal, source, window], |r| r.get::<_, i64>(0))
            .optional()
            .map(|o| o.unwrap_or(0).max(0) as u64)
    }

    fn held(conn: &Connection, principal: &str, source: &str, window: i64) -> rusqlite::Result<(u64, u32)> {
        conn.prepare_cached("SELECT coalesce(sum(amount), 0), count(*) FROM holds WHERE principal = ?1 AND source = ?2 AND window = ?3")?
            .query_row(params![principal, source, window], |r| Ok((r.get::<_, i64>(0)?.max(0) as u64, r.get::<_, i64>(1)? as u32)))
    }

    /// Reserves `amount` of `source` for `who` at `now` — one transaction:
    /// the expired swept, then refused if the box is full (`box_used`, past
    /// the full line, for anyone not exempt), if as many reservations of the
    /// source are open as the plan allows at once, or if used + held +
    /// `amount` would pass the cap; else held until `now` + the hold time.
    pub fn reserve(&self, who: &Who, source: &str, amount: u64, now: i64, box_used: Option<f64>) -> Result<Reserved, Refusal> {
        let src = known(source)?;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(LedgerError::from)?;
        tx.execute("DELETE FROM holds WHERE expires <= ?1", [now]).map_err(LedgerError::from)?;
        if !who.exempt {
            if let Some(u) = box_used.filter(|u| *u >= self.settings.box_lines.full_percent) {
                return Err(Refusal::BoxFull { used_percent: u, full_percent: self.settings.box_lines.full_percent });
            }
        }
        let window = src.window.at(now);
        let (held, open) = Self::held(&tx, &who.principal, source, window).map_err(LedgerError::from)?;
        if let Some(max) = self.settings.concurrent.get(source).copied().filter(|_| !who.exempt) {
            if open >= max {
                return Err(Refusal::TooManyAtOnce { source: source.to_string(), open, max });
            }
        }
        let used = Self::used(&tx, &who.principal, source, window).map_err(LedgerError::from)?;
        let (cap, _) = Self::cap_of(&tx, who, source).map_err(LedgerError::from)?;
        let after = used.saturating_add(held).saturating_add(amount);
        let remaining = match cap {
            Cap::At(c) if after > c => return Err(Refusal::OverAllowance { source: source.to_string(), used: used + held, cap: c, asked: amount }),
            Cap::At(c) => Some(c - after),
            Cap::Unlimited => None,
        };
        let ticket = uuid::Uuid::new_v4().simple().to_string();
        let expires = now + self.settings.hold_secs;
        tx.execute(
            "INSERT INTO holds (ticket, principal, source, window, amount, at, expires) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![ticket, who.principal, source, window, amount as i64, now, expires],
        )
        .map_err(LedgerError::from)?;
        tx.commit().map_err(LedgerError::from)?;
        Ok(Reserved { ticket, remaining, expires })
    }

    /// Commits reservation `ticket`: `amount` (what was actually written —
    /// at most what was reserved; none: all of it) moves into usage, the
    /// hold goes, in one transaction.
    pub fn commit(&self, ticket: &str, amount: Option<u64>) -> Result<Committed, LedgerError> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let hold: Option<(String, String, i64, i64)> = tx
            .query_row("SELECT principal, source, window, amount FROM holds WHERE ticket = ?1", [ticket], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?;
        let (principal, source, window, reserved) = hold.ok_or_else(|| LedgerError::NoHold(ticket.to_string()))?;
        let reserved = reserved.max(0) as u64;
        let amount = amount.unwrap_or(reserved);
        if amount > reserved {
            return Err(LedgerError::PastHold { ticket: ticket.to_string(), reserved, amount });
        }
        tx.execute("DELETE FROM holds WHERE ticket = ?1", [ticket])?;
        tx.execute(
            "INSERT INTO usage (principal, source, window, used) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (principal, source, window) DO UPDATE SET used = used + excluded.used",
            params![principal, source, window, amount as i64],
        )?;
        let used = Self::used(&tx, &principal, &source, window)?;
        tx.commit()?;
        Ok(Committed { principal, source, amount, used })
    }

    /// Releases reservation `ticket`: nothing is counted. Its (principal, source, amount).
    pub fn release(&self, ticket: &str) -> Result<(String, String, u64), LedgerError> {
        let conn = self.lock();
        let hold: Option<(String, String, i64)> =
            conn.query_row("SELECT principal, source, amount FROM holds WHERE ticket = ?1", [ticket], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        let (p, s, a) = hold.ok_or_else(|| LedgerError::NoHold(ticket.to_string()))?;
        conn.execute("DELETE FROM holds WHERE ticket = ?1", [ticket])?;
        Ok((p, s, a.max(0) as u64))
    }

    /// Gives `amount` of `source` back to `principal` (a deletion) — never
    /// below nothing; in the current window for a windowed source. Used, after.
    pub fn credit(&self, principal: &str, source: &str, amount: u64, now: i64) -> Result<u64, LedgerError> {
        let src = known(source)?;
        let window = src.window.at(now);
        let conn = self.lock();
        conn.execute(
            "UPDATE usage SET used = max(0, used - ?4) WHERE principal = ?1 AND source = ?2 AND window = ?3",
            params![principal, source, window, amount.min(i64::MAX as u64) as i64],
        )?;
        Ok(Self::used(&conn, principal, source, window)?)
    }

    /// Removes every reservation expired by `now`, and windows a week old.
    /// How many reservations went.
    pub fn sweep(&self, now: i64) -> Result<usize, LedgerError> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM holds WHERE expires <= ?1", [now])?;
        let week_ago = sources::Window::Day.at(now) - 7;
        conn.execute("DELETE FROM usage WHERE window > 0 AND window < ?1", [week_ago])?;
        Ok(n)
    }

    /// `who`'s usage of every source at `now`: used, held, the cap and where it came from.
    pub fn usage(&self, who: &Who, now: i64) -> Result<Vec<Line>, LedgerError> {
        let conn = self.lock();
        let mut lines = Vec::new();
        for source in sources::SOURCES {
            let w = source.window.at(now);
            let used = Self::used(&conn, &who.principal, source.name, w)?;
            let (held, _) = Self::held(&conn, &who.principal, source.name, w)?;
            let (cap, from) = Self::cap_of(&conn, who, source.name)?;
            lines.push(Line { source, used, held, cap, from });
        }
        Ok(lines)
    }

    /// Every principal the ledger knows: one with usage, a reservation, or an allowance of its own.
    pub fn principals(&self) -> Result<Vec<String>, LedgerError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT principal FROM usage UNION SELECT principal FROM holds UNION SELECT who FROM allowances WHERE kind = 'principal' ORDER BY 1",
        )?;
        let out = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(out)
    }

    /// The teams holding an allowance — the only ones whose members the
    /// service needs to know.
    pub fn teams_with_allowances(&self) -> Result<Vec<String>, LedgerError> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT DISTINCT who FROM allowances WHERE kind = 'team' ORDER BY 1")?;
        let out = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(out)
    }

    /// Every allowance: (holder, source, cap).
    pub fn allowances(&self) -> Result<Vec<(Holder, String, Cap)>, LedgerError> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT kind, who, source, cap FROM allowances ORDER BY kind, who, source")?;
        let out = stmt
            .query_map([], |r| {
                let (kind, who): (String, String) = (r.get(0)?, r.get(1)?);
                let h = match kind.as_str() {
                    "principal" => Holder::Principal(who),
                    "team" => Holder::Team(who),
                    _ => Holder::Default,
                };
                Ok((h, r.get::<_, String>(2)?, Cap::of_stored(r.get(3)?)))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(out)
    }

    /// Sets `holder`'s allowance on `source` to `cap`; `None` removes it (a
    /// principal or team then falls back; the default then is unlimited). A
    /// cap below what is used blocks new writes and deletes nothing.
    pub fn set(&self, holder: &Holder, source: &str, cap: Option<Cap>, by: &str) -> Result<(), LedgerError> {
        known(source)?;
        let (kind, who) = holder.columns();
        let conn = self.lock();
        match cap {
            Some(c) => conn.execute(
                "INSERT INTO allowances (kind, who, source, cap, set_by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (kind, who, source) DO UPDATE SET cap = excluded.cap, set_by = excluded.set_by, at = excluded.at",
                params![kind, who, source, c.stored(), by, now()],
            )?,
            None => conn.execute("DELETE FROM allowances WHERE kind = ?1 AND who = ?2 AND source = ?3", params![kind, who, source])?,
        };
        Ok(())
    }

    /// Recount: `principal`'s total usage of `source` is `counted`, from the
    /// store that keeps it. What it was.
    pub fn replace_total(&self, principal: &str, source: &str, counted: u64) -> Result<u64, LedgerError> {
        known(source)?;
        let conn = self.lock();
        let was = Self::used(&conn, principal, source, 0)?;
        conn.execute(
            "INSERT INTO usage (principal, source, window, used) VALUES (?1, ?2, 0, ?3)
             ON CONFLICT (principal, source, window) DO UPDATE SET used = excluded.used",
            params![principal, source, counted.min(i64::MAX as u64) as i64],
        )?;
        Ok(was)
    }

    /// Every principal with a total of `source` recorded, and it.
    pub fn totals(&self, source: &str) -> Result<Vec<(String, u64)>, LedgerError> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT principal, used FROM usage WHERE source = ?1 AND window = 0 ORDER BY principal")?;
        let out = stmt.query_map([source], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as u64)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(out)
    }

    /// How many reservations are open — for a log line and the tests.
    pub fn open_holds(&self) -> usize {
        self.lock().query_row("SELECT count(*) FROM holds", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }
}

#[cfg(test)]
mod tests;
