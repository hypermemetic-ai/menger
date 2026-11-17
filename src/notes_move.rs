//! NOTES-3: **documents become notes** — what the release before kept under
//! the `docs` service's names, moved to the `notes` service's at the first
//! start of the release that carries the rename. A step of the start, as
//! the backfill is ([`crate::backfill`]): every later start finds nothing
//! to do, and writes nothing.
//!
//! - **The file** ([`beside`]): where `--notes-db-path` names none, the
//!   file is `notes.db` beside the tickets file (or `<stem>.notes.db`). When
//!   it is absent and its twin `docs.db` (`<stem>.docs.db`) is there, the
//!   twin is renamed to it — its `-wal` and `-shm` with it — before anything
//!   opens it, and a relative symlink is left at the old name pointing at
//!   the new one, so the release before opens the same data. The tables
//!   inside keep their names (`docs`, `doc_log`, `doc_threads`).
//! - **Quota** is the quota service's own file: `menger_quota`'s ledger
//!   rekeys `docs.bytes` and `docs.count` as it opens
//!   (`quota/src/ledger/renamed.rs`).
//! - **Stored roles** ([`roles`]): an admin's edit of a role (`auth.role_set`,
//!   the tickets store's `roles` table) that grants `docs.<method>` grants
//!   `notes.<method>` too. The store keeps a grant naming a method no
//!   service has (it is read as words, and holds nothing), so the old grant
//!   stays beside the new one and the release before still works.

use menger_tickets_api::store::{Store, System};
use std::path::{Path, PathBuf};

/// The service's file before NOTES-3, and the prefix of its grants and
/// quota sources.
pub const OLD_FILE: &str = "docs.db";
const OLD: &str = "docs.";
const NEW: &str = "notes.";

/// The notes file beside the tickets file `db_path` (`notes.db`, or
/// `<stem>.notes.db`), its `docs` twin moved to it first when it is the
/// only one there. `Ok(path)` whether or not anything moved.
pub fn beside(db_path: &Path) -> anyhow::Result<PathBuf> {
    let notes = crate::default_notes_db_path(db_path);
    let docs = crate::builder::sibling_db_path(db_path, OLD_FILE);
    if move_file(&docs, &notes)? {
        tracing::info!("notes:     moved {} to {} (NOTES-3; a link left at the old name)", docs.display(), notes.display());
    }
    Ok(notes)
}

/// `docs` renamed to `notes` with its `-wal` and `-shm`, and `docs` left a
/// relative symlink to `notes` — when `notes` is absent (nor a dangling
/// link) and `docs` is a file. `true`: it moved.
pub fn move_file(docs: &Path, notes: &Path) -> anyhow::Result<bool> {
    if notes.symlink_metadata().is_ok() || !docs.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return Ok(false);
    }
    let with = |p: &Path, s: &str| PathBuf::from(format!("{}{s}", p.display()));
    // The sidecars first: a crash between leaves the main file at its old
    // name, found again next start, with its journal beside the new one —
    // so the main file goes last, and its sidecars are moved back if it fails.
    let mut moved = Vec::new();
    for s in ["-wal", "-shm"] {
        let (from, to) = (with(docs, s), with(notes, s));
        if from.symlink_metadata().is_ok() {
            std::fs::rename(&from, &to).map_err(|e| anyhow::anyhow!("{} → {}: {e}", from.display(), to.display()))?;
            moved.push((from, to));
        }
    }
    if let Err(e) = std::fs::rename(docs, notes) {
        for (from, to) in moved {
            let _ = std::fs::rename(to, from);
        }
        anyhow::bail!("{} → {}: {e}", docs.display(), notes.display());
    }
    let target = notes.file_name().ok_or_else(|| anyhow::anyhow!("{}: no file name", notes.display()))?;
    let link = match docs.parent().zip(notes.parent()) {
        Some((a, b)) if a == b => PathBuf::from(target),
        _ => notes.to_path_buf(),
    };
    std::os::unix::fs::symlink(&link, docs).map_err(|e| anyhow::anyhow!("{} → {}: {e}", docs.display(), link.display()))?;
    Ok(true)
}

/// The `notes.<method>` each stored role lacks beside a `docs.<method>` it
/// grants, added — the old grant kept. How many roles were written (none on
/// a second start).
pub async fn roles(store: &Store, system: &System) -> anyhow::Result<usize> {
    let mut written = 0;
    for (role, grants) in store.role_edits() {
        let Some(more) = renamed(&grants) else { continue };
        store.set_role(system, role.clone(), more).await.map_err(|e| anyhow::anyhow!("the role {role}: {e}"))?;
        tracing::info!("notes:     the stored role {role} grants notes.* beside its docs.* (NOTES-3)");
        written += 1;
    }
    Ok(written)
}

/// `grants` with a `notes.<method>` after each `docs.<method>` it lacks;
/// `None` when it lacks none.
pub fn renamed(grants: &[String]) -> Option<Vec<String>> {
    let mut out = grants.to_vec();
    for g in grants {
        if let Some(method) = g.strip_prefix(OLD) {
            let new = format!("{NEW}{method}");
            if !out.contains(&new) {
                out.push(new);
            }
        }
    }
    (out.len() != grants.len()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_gains_the_new_grant_beside_the_old_once() {
        let g = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(renamed(&g(&["tickets.get", "docs.save", "docs.squash"])), Some(g(&["tickets.get", "docs.save", "docs.squash", "notes.save", "notes.squash"])));
        assert_eq!(renamed(&g(&["tickets.get", "docs.save", "docs.squash", "notes.save", "notes.squash"])), None, "a second start writes nothing");
        assert_eq!(renamed(&g(&["tickets.get"])), None);
    }

    /// The file with its journal: written by the store and left open — a
    /// stop mid-write, its `-wal` holding what the main file does not yet —
    /// copied as the release before left it; moved with its sidecars; a
    /// relative link left; the same note read through either name; a second
    /// move does nothing.
    #[test]
    fn the_file_moves_with_its_journal_and_leaves_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let (docs, notes) = (dir.path().join("docs.db"), dir.path().join("notes.db"));
        let live = dir.path().join("live.db");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let id = rt.block_on(async {
            let s = std::sync::Arc::new(menger_notes::NotesStore::open(&live).unwrap());
            let w = s.create("acme", "The kiln", "Fire twice a week.\n", "builder", "key:aa").await.unwrap();
            for s in ["", "-wal", "-shm"] {
                std::fs::copy(format!("{}{s}", live.display()), format!("{}{s}", docs.display())).unwrap();
            }
            w.id
        });
        assert!(PathBuf::from(format!("{}-wal", docs.display())).exists(), "a journal to carry");
        assert!(move_file(&docs, &notes).unwrap());
        for s in ["-wal", "-shm"] {
            assert!(PathBuf::from(format!("{}{s}", notes.display())).exists() && !PathBuf::from(format!("{}{s}", docs.display())).exists(), "{s} moved");
        }
        assert_eq!(std::fs::read_link(&docs).unwrap(), Path::new("notes.db"));
        assert!(!move_file(&docs, &notes).unwrap(), "the second start moves nothing");
        rt.block_on(async {
            for p in [&notes, &docs] {
                let s = std::sync::Arc::new(menger_notes::NotesStore::open(p).unwrap());
                assert_eq!(s.get(&id).await.unwrap().body.as_deref(), Some("Fire twice a week.\n"), "{}", p.display());
            }
        });
        // Nothing there: nothing moves, nothing is made.
        let empty = tempfile::tempdir().unwrap();
        assert!(!move_file(&empty.path().join("docs.db"), &empty.path().join("notes.db")).unwrap());
        assert_eq!(std::fs::read_dir(empty.path()).unwrap().count(), 0);
    }

    /// A tickets file the data directory does not name gives its stem to both.
    #[test]
    fn beside_a_scratch_database_the_stemmed_twin_moves() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("menger-x.docs.db"), b"").unwrap();
        let at = beside(&dir.path().join("menger-x.db")).unwrap();
        assert_eq!(at, dir.path().join("menger-x.notes.db"));
        assert!(at.is_file());
        assert_eq!(std::fs::read_link(dir.path().join("menger-x.docs.db")).unwrap(), Path::new("menger-x.notes.db"));
    }
}
