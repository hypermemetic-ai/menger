//! V2-70: **the ledger's history** — one point per principal per source per
//! UTC day, in `quota.db`'s `history` table: a total source's amount at the
//! day's end, a day's counter its day's total. Every change writes today's
//! point (a commit, a credit, a recount's total), so the last write of a day
//! is its end and today's point is now; a day nothing moved has no row, and
//! a total is carried from the last day that has one.
//!
//! **Backfilled** where the past is knowable (the recount asks each store
//! by day — media's files by when each was first kept, documents' changes,
//! tickets' revisions): the days before a principal's first point of a
//! source, marked `estimate` — what the stores keep now, laid back along
//! when it was written; what was deleted since is not in it.

use super::{known, Ledger, LedgerError};
use crate::sources::{self, Window};
use rusqlite::{params, Connection, OptionalExtension};

/// Seconds in a UTC day.
pub const DAY: i64 = 86_400;

/// The most days `history` answers (two years).
pub const MAX_DAYS: u32 = 730;

pub const TABLE: &str = "
    CREATE TABLE IF NOT EXISTS history (
        principal TEXT NOT NULL,
        source    TEXT NOT NULL,
        day       INTEGER NOT NULL,
        used      INTEGER NOT NULL,
        estimate  INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (principal, source, day)
    );
";

/// Whose history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whose {
    One(String),
    /// The system's: every principal's, summed per day.
    Everyone,
}

/// One day of one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    /// Days since 1970-01-01 (UTC).
    pub day: i64,
    pub source: &'static str,
    pub used: u64,
    /// Backfilled from the stores, not counted as it happened.
    pub estimate: bool,
}

/// Writes `principal`'s point of `source` for the day `now` falls in (a
/// total) or the counter's own day (`window`, a day's): `used`, as counted.
pub(super) fn record(conn: &Connection, principal: &str, source: &str, window: i64, used: u64, now: i64) -> rusqlite::Result<()> {
    let day = if window > 0 { window } else { now.div_euclid(DAY) };
    conn.execute(
        "INSERT INTO history (principal, source, day, used, estimate) VALUES (?1, ?2, ?3, ?4, 0)
         ON CONFLICT (principal, source, day) DO UPDATE SET used = excluded.used, estimate = 0",
        params![principal, source, day, used.min(i64::MAX as u64) as i64],
    )?;
    Ok(())
}

/// `YYYY-MM-DD` of `day` (days since 1970-01-01) — Hinnant's civil-from-days.
pub fn date(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl Ledger {
    /// The last `days` days to `now`'s, oldest first, of `source` (none:
    /// every source) for `whose`: a point per source per day — a total
    /// carried from its last recorded day, a day's counter nothing on a day
    /// with no row; for everyone, summed over every principal.
    pub fn history(&self, whose: &Whose, source: Option<&str>, days: u32, now: i64) -> Result<Vec<Point>, LedgerError> {
        if let Some(s) = source {
            known(s)?;
        }
        let today = now.div_euclid(DAY);
        let n = days.clamp(1, MAX_DAYS) as usize;
        let start = today - n as i64 + 1;
        let conn = self.lock();
        let mut out = Vec::new();
        for src in sources::SOURCES.iter().filter(|s| source.is_none_or(|w| w == s.name)) {
            let mut sum = vec![(0u64, false); n];
            let mut stmt = conn.prepare_cached(
                "SELECT principal, day, used, estimate FROM history WHERE source = ?1 AND day <= ?2 AND (?3 IS NULL OR principal = ?3) ORDER BY principal, day",
            )?;
            let who = match whose {
                Whose::One(p) => Some(p.clone()),
                Whose::Everyone => None,
            };
            let rows = stmt.query_map(params![src.name, today, who], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?.max(0) as u64, r.get::<_, i64>(3)? != 0)))?;
            let rows = rows.collect::<Result<Vec<_>, _>>()?;
            for chunk in rows.chunk_by(|a, b| a.0 == b.0) {
                fill(&mut sum, chunk.iter().map(|(_, d, u, e)| (*d, *u, *e)), start, src.window);
            }
            out.extend(sum.into_iter().enumerate().map(|(i, (used, estimate))| Point { day: start + i as i64, source: src.name, used, estimate }));
        }
        Ok(out)
    }

    /// Lays `principal`'s past of `source` (a total) back: each `(day,
    /// amount at that day's end)` of `cumulative` before the first day the
    /// ledger counted it (or today) is kept as an estimate; a day kept
    /// already is left. How many points were written.
    pub fn backfill(&self, principal: &str, source: &str, cumulative: &[(i64, u64)], now: i64) -> Result<usize, LedgerError> {
        known(source)?;
        let today = now.div_euclid(DAY);
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let first: Option<i64> = tx
            .query_row("SELECT min(day) FROM history WHERE principal = ?1 AND source = ?2 AND estimate = 0", params![principal, source], |r| r.get(0))
            .optional()?
            .flatten();
        let before = first.unwrap_or(today).min(today);
        let mut n = 0;
        for (day, used) in cumulative.iter().filter(|(d, _)| *d < before) {
            n += tx.execute(
                "INSERT OR IGNORE INTO history (principal, source, day, used, estimate) VALUES (?1, ?2, ?3, ?4, 1)",
                params![principal, source, day, (*used).min(i64::MAX as u64) as i64],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }
}

/// Adds one principal's rows (by day, ascending) into `sum` over the days
/// from `start`: a total carried forward from its last row, a day's counter
/// on its own day alone.
fn fill(sum: &mut [(u64, bool)], rows: impl Iterator<Item = (i64, u64, bool)>, start: i64, window: Window) {
    let mut rows = rows.peekable();
    let mut cur = (0u64, false);
    for (i, slot) in sum.iter_mut().enumerate() {
        let day = start + i as i64;
        let mut today = None;
        while let Some(&(d, u, e)) = rows.peek() {
            if d > day {
                break;
            }
            match window {
                Window::Total => cur = (u, e),
                Window::Day if d == day => today = Some((u, e)),
                Window::Day => {}
            }
            rows.next();
        }
        let (u, e) = match window {
            Window::Total => cur,
            Window::Day => today.unwrap_or((0, false)),
        };
        slot.0 += u;
        slot.1 |= e && u > 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Who;
    use crate::plan::Plan;

    fn ledger(dir: &tempfile::TempDir) -> Ledger {
        Ledger::open(&dir.path().join("quota.db"), &Plan::built_in()).unwrap()
    }

    fn of<'a>(pts: &'a [Point], source: &str) -> Vec<(i64, u64, bool)> {
        pts.iter().filter(|p| p.source == source).map(|p| (p.day, p.used, p.estimate)).collect()
    }

    #[test]
    fn dates_are_utc_days() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(20_767), "2026-11-10");
        assert_eq!(date(11_016), "2000-02-29");
        assert_eq!(date(-1), "1969-12-31");
    }

    /// Every commit, credit and recount writes the day's point; a total is
    /// carried over the days nothing moved; a day's counter is its day's
    /// alone; everyone's is the sum; a backfill lays the past before the
    /// first counted day, an estimate, and never over a counted day.
    #[test]
    fn a_point_a_day_carried_summed_and_backfilled() {
        let dir = tempfile::tempdir().unwrap();
        let l = ledger(&dir);
        let d0 = 20_000i64;
        let at = |d: i64| d * DAY + 3600;
        let (a, b) = (Who::plain("key:a"), Who::plain("key:b"));
        let r = l.reserve(&a, "media.bytes", 100, at(d0), None).unwrap();
        l.commit_at(&r.ticket, None, at(d0)).unwrap();
        let r = l.reserve(&a, "media.bytes", 50, at(d0 + 2), None).unwrap();
        l.commit_at(&r.ticket, None, at(d0 + 2)).unwrap();
        l.credit(&a.principal, "media.bytes", 30, at(d0 + 3)).unwrap();
        let r = l.reserve(&b, "media.bytes", 7, at(d0 + 1), None).unwrap();
        l.commit_at(&r.ticket, None, at(d0 + 1)).unwrap();
        for d in [d0, d0 + 2] {
            let r = l.reserve(&a, "media.fetch.calls", 1, at(d), None).unwrap();
            l.commit_at(&r.ticket, None, at(d)).unwrap();
        }
        let pts = l.history(&Whose::One("key:a".into()), None, 5, at(d0 + 3)).unwrap();
        assert_eq!(pts.len(), 5 * sources::SOURCES.len(), "every source, every day");
        assert_eq!(of(&pts, "media.bytes"), vec![(d0 - 1, 0, false), (d0, 100, false), (d0 + 1, 100, false), (d0 + 2, 150, false), (d0 + 3, 120, false)]);
        assert_eq!(of(&pts, "media.fetch.calls").iter().map(|p| p.1).collect::<Vec<_>>(), vec![0, 1, 0, 1, 0]);
        let all = l.history(&Whose::Everyone, Some("media.bytes"), 3, at(d0 + 3)).unwrap();
        assert_eq!(of(&all, "media.bytes").iter().map(|p| p.1).collect::<Vec<_>>(), vec![107, 157, 127]);
        // A recount writes the day's point too.
        l.replace_total_at("key:b", "media.bytes", 9, at(d0 + 3)).unwrap();
        assert_eq!(of(&l.history(&Whose::One("key:b".into()), Some("media.bytes"), 1, at(d0 + 3)).unwrap(), "media.bytes"), vec![(d0 + 3, 9, false)]);
        // Backfill: only before the first counted day (d0 for a); a day kept is left.
        let n = l.backfill("key:a", "media.bytes", &[(d0 - 3, 10), (d0 - 1, 60), (d0, 999), (d0 + 1, 999)], at(d0 + 3)).unwrap();
        assert_eq!(n, 2);
        let pts = l.history(&Whose::One("key:a".into()), Some("media.bytes"), 6, at(d0 + 3)).unwrap();
        assert_eq!(of(&pts, "media.bytes")[..3], [(d0 - 2, 10, true), (d0 - 1, 60, true), (d0, 100, false)]);
        assert_eq!(l.backfill("key:a", "media.bytes", &[(d0 - 3, 10), (d0 - 1, 60)], at(d0 + 3)).unwrap(), 0, "again: nothing");
        // No counted day yet: everything before today.
        assert_eq!(l.backfill("key:c", "docs.bytes", &[(d0 + 1, 5), (d0 + 3, 6)], at(d0 + 3)).unwrap(), 1);
        assert!(matches!(l.history(&Whose::Everyone, Some("nope"), 3, at(d0)), Err(LedgerError::UnknownSource(_))));
    }
}
