//! NOTES-3: **the sources that changed their names, rekeyed** — the docs
//! service became the notes service, so `docs.bytes` and `docs.count` are
//! `notes.bytes` and `notes.count` ([`crate::sources::RENAMED`]). At the
//! first open of a ledger that holds rows under an old name, every one of
//! them moves to the new name in one transaction: usage, open holds, caps
//! (a principal's, a team's, the default) and history; the plan's
//! `concurrent` setting too. A ledger with none writes nothing.
//!
//! **Both names at once** happens only after a rollback: the release
//! before counts its documents from zero under the old name beside the
//! rows already moved. Opened again here, the old rows are folded in:
//! usage and history added to the new name's, a cap the newer of the two
//! (by when it was set), a hold rekeyed as it is.

use crate::sources::RENAMED;
use rusqlite::{params, Connection, OptionalExtension};

/// The rows under an old name moved to the new, once; how many rows moved
/// (0: nothing was written).
pub fn rekey(conn: &mut Connection) -> rusqlite::Result<usize> {
    let concurrent: Option<String> = conn.query_row("SELECT value FROM settings WHERE key = 'concurrent'", [], |r| r.get(0)).optional()?;
    let moved_setting = concurrent.as_deref().and_then(renamed_concurrent);
    let mut waiting = 0usize;
    for (old, _) in RENAMED {
        for table in ["usage", "holds", "allowances", "history"] {
            waiting += conn.query_row(&format!("SELECT count(*) FROM {table} WHERE source = ?1"), [old], |r| r.get::<_, i64>(0))? as usize;
        }
    }
    if waiting == 0 && moved_setting.is_none() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    for (old, new) in RENAMED {
        tx.execute(
            "INSERT INTO usage (principal, source, window, used) SELECT principal, ?2, window, used FROM usage WHERE source = ?1
                 ON CONFLICT (principal, source, window) DO UPDATE SET used = used + excluded.used",
            params![old, new],
        )?;
        tx.execute(
            "INSERT INTO history (principal, source, day, used, estimate) SELECT principal, ?2, day, used, estimate FROM history WHERE source = ?1
                 ON CONFLICT (principal, source, day) DO UPDATE SET used = used + excluded.used",
            params![old, new],
        )?;
        tx.execute(
            "INSERT INTO allowances (kind, who, source, cap, set_by, at) SELECT kind, who, ?2, cap, set_by, at FROM allowances WHERE source = ?1
                 ON CONFLICT (kind, who, source) DO UPDATE SET cap = excluded.cap, set_by = excluded.set_by, at = excluded.at WHERE excluded.at > allowances.at",
            params![old, new],
        )?;
        tx.execute("UPDATE holds SET source = ?2 WHERE source = ?1", params![old, new])?;
        for table in ["usage", "history", "allowances"] {
            tx.execute(&format!("DELETE FROM {table} WHERE source = ?1"), [old])?;
        }
    }
    if let Some(v) = &moved_setting {
        tx.execute("UPDATE settings SET value = ?1 WHERE key = 'concurrent'", [v])?;
    }
    tx.commit()?;
    Ok(waiting + usize::from(moved_setting.is_some()))
}

/// The plan's `concurrent` setting (JSON, source → count) with its old
/// names renamed; `None` when it names none.
fn renamed_concurrent(json: &str) -> Option<String> {
    let map: std::collections::BTreeMap<String, u32> = serde_json::from_str(json).ok()?;
    if !map.keys().any(|k| RENAMED.iter().any(|(old, _)| old == k)) {
        return None;
    }
    let map: std::collections::BTreeMap<String, u32> = map.into_iter().map(|(k, n)| (crate::sources::renamed(&k).to_string(), n)).collect();
    serde_json::to_string(&map).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{history, Ledger, TABLES};
    use crate::plan::Plan;

    /// A ledger the release before wrote, by hand from its schema: the plan's
    /// defaults under `docs.*`, a principal's usage, cap, open hold and
    /// history under them, and a `concurrent` setting naming one.
    fn before(path: &std::path::Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(TABLES).unwrap();
        c.execute_batch(history::TABLE).unwrap();
        c.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('plan', 'free-1k'), ('concurrent', '{\"docs.bytes\":3,\"media.bytes\":2}'), ('hold_secs', '3600');
             INSERT INTO allowances VALUES ('default', '', 'docs.bytes', 200000000, 'plan:free-1k', 100), ('default', '', 'docs.count', NULL, 'plan:free-1k', 100),
                                           ('principal', 'key:aa', 'docs.bytes', 10000000, 'key:admin', 200), ('default', '', 'media.bytes', 2000000000, 'plan:free-1k', 100);
             INSERT INTO usage VALUES ('key:aa', 'docs.bytes', 0, 1234), ('key:aa', 'docs.count', 0, 2), ('key:aa', 'media.bytes', 0, 9);
             INSERT INTO holds VALUES ('h1', 'key:aa', 'docs.bytes', 0, 10, 300, 4000000000);
             INSERT INTO history (principal, source, day, used) VALUES ('key:aa', 'docs.bytes', 20000, 1234);",
        )
        .unwrap();
    }

    fn rows(path: &std::path::Path) -> Vec<String> {
        let c = Connection::open(path).unwrap();
        let mut out = Vec::new();
        for (t, cols) in [("usage", "principal, source, window, used"), ("allowances", "kind, who, source, cap, at"), ("holds", "ticket, source, amount"), ("history", "principal, source, day, used")] {
            let mut st = c.prepare(&format!("SELECT {cols} FROM {t} ORDER BY 1, 2, 3")).unwrap();
            let n = st.column_count();
            out.extend(st.query_map([], |r| Ok((0..n).map(|i| format!("{:?}", r.get::<_, rusqlite::types::Value>(i).unwrap())).collect::<Vec<_>>().join("|"))).unwrap().map(|r| format!("{t}:{}", r.unwrap())));
        }
        out
    }

    #[test]
    fn the_old_sources_move_once_and_the_ledger_reads_them_as_the_new() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.db");
        before(&path);
        let l = Ledger::open(&path, &Plan::built_in()).unwrap();
        assert_eq!(l.settings().concurrent.get("notes.bytes"), Some(&3), "the setting renamed");
        drop(l);
        let after = rows(&path);
        assert!(after.iter().all(|r| !r.contains("docs.")), "{after:?}");
        for want in [
            "usage:Text(\"key:aa\")|Text(\"notes.bytes\")|Integer(0)|Integer(1234)",
            "usage:Text(\"key:aa\")|Text(\"notes.count\")|Integer(0)|Integer(2)",
            "allowances:Text(\"principal\")|Text(\"key:aa\")|Text(\"notes.bytes\")|Integer(10000000)|Integer(200)",
            "allowances:Text(\"default\")|Text(\"\")|Text(\"notes.count\")|Null|Integer(100)",
            "holds:Text(\"h1\")|Text(\"notes.bytes\")|Integer(10)",
            "history:Text(\"key:aa\")|Text(\"notes.bytes\")|Integer(20000)|Integer(1234)",
        ] {
            assert!(after.iter().any(|r| r == want), "{want} in {after:?}");
        }
        // The second open: nothing to move, nothing written.
        assert_eq!(rekey(&mut Connection::open(&path).unwrap()).unwrap(), 0);
        Ledger::open(&path, &Plan::built_in()).unwrap();
        assert_eq!(rows(&path), after);
    }

    /// After a rollback the release before counted under the old name again,
    /// from zero, beside the rows already moved: its usage is added, its cap
    /// wins only if it was set later.
    #[test]
    fn rows_under_both_names_fold_into_the_new() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.db");
        before(&path);
        drop(Ledger::open(&path, &Plan::built_in()).unwrap());
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "INSERT INTO usage VALUES ('key:aa', 'docs.bytes', 0, 66);
                 INSERT INTO allowances VALUES ('principal', 'key:aa', 'docs.bytes', 5, 'key:admin', 150), ('principal', 'key:bb', 'docs.count', 7, 'key:admin', 300);",
            )
            .unwrap();
        drop(Ledger::open(&path, &Plan::built_in()).unwrap());
        let after = rows(&path);
        assert!(after.iter().all(|r| !r.contains("docs.")), "{after:?}");
        assert!(after.contains(&"usage:Text(\"key:aa\")|Text(\"notes.bytes\")|Integer(0)|Integer(1300)".to_string()), "{after:?}");
        assert!(after.contains(&"allowances:Text(\"principal\")|Text(\"key:aa\")|Text(\"notes.bytes\")|Integer(10000000)|Integer(200)".to_string()), "the later cap kept: {after:?}");
        assert!(after.contains(&"allowances:Text(\"principal\")|Text(\"key:bb\")|Text(\"notes.count\")|Integer(7)|Integer(300)".to_string()), "{after:?}");
    }

    #[test]
    fn a_plan_naming_the_old_sources_reads_as_the_new() {
        let p = Plan::parse(r#"{"name": "old", "default": {"docs.bytes": "1 MB", "docs.count": 5}, "concurrent": {"docs.bytes": 1}}"#).unwrap();
        assert_eq!(p.default.keys().collect::<Vec<_>>(), ["notes.bytes", "notes.count"]);
        assert_eq!(p.concurrent.keys().collect::<Vec<_>>(), ["notes.bytes"]);
        assert!(crate::sources::source("docs.bytes").is_none(), "only a plan reads the old name");
    }
}
