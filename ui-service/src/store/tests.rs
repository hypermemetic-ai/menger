//! UI-3: the ui store's own tests — the testing surface's first six rows.
//! Each opens a throwaway `ui.db` and reads the file back through a
//! connection of its own, so what is asserted is what the file holds.

use super::*;
use rusqlite::types::Value;
use sha2::{Digest, Sha256};

/// A small bundle: a tar with `index.html` (saying `n`) and one asset.
fn bundle(n: u32) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for (name, body) in [
        ("index.html", format!("<!doctype html><title>v{n}</title><p>version {n} on {{{{WS_PORT}}}}</p>")),
        ("app.js", format!("console.log({n});")),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn version(n: u32, at: i64) -> NewVersion {
    NewVersion {
        bundle: bundle(n),
        // UI-9: the contract's JSON (UI-4).
        manifest: serde_json::json!({ "abi": format!("a{n}"), "contract": format!("c{n}"), "routes": {}, "components": {}, "slots": {} }).to_string(),
        commit: Some(format!("{n:07x}")),
        pushed_by: "key:admin".into(),
        pushed_at: at,
    }
}

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// Every row of `table`, as the file holds it, in a fixed order.
fn rows(path: &Path, table: &str) -> Vec<Vec<Value>> {
    let c = Connection::open(path).unwrap();
    let order = if table == "ui_versions" { "hash" } else if table == "ui_pointer" { "name" } else { "principal" };
    let mut q = c.prepare(&format!("SELECT * FROM {table} ORDER BY {order}")).unwrap();
    let n = q.column_count();
    q.query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i)).collect()).unwrap().map(Result::unwrap).collect()
}

#[test]
fn a_push_stores_once_per_hash() {
    let dir = tmp();
    let path = dir.path().join(UI_DB_FILE);
    let s = UiStore::open(&path).unwrap();
    let first = s.push(version(1, 100)).unwrap();
    assert_eq!(first, Pushed { version: sha(&bundle(1)), uploaded: sha(&bundle(1)), live: true, new: true });
    let row_one = rows(&path, "ui_versions").into_iter().find(|r| r[0] == Value::Text(first.version.clone())).unwrap();
    let second = s.push(version(2, 200)).unwrap();
    assert_eq!(second, Pushed { version: sha(&bundle(2)), uploaded: sha(&bundle(2)), live: true, new: true });
    let mut later = version(1, 300);
    later.pushed_by = "key:someone-else".into();
    let again = s.push(later).unwrap();
    assert_eq!(again, Pushed { version: sha(&bundle(1)), uploaded: sha(&bundle(1)), live: true, new: false }, "the same bytes are the same version");
    let all = rows(&path, "ui_versions");
    assert_eq!(all.len(), 2, "two rows");
    let row = all.iter().find(|r| r[0] == Value::Text(first.version.clone())).unwrap();
    assert_eq!(row, &row_one, "row 1 as first pushed: its pushed_at and pushed_by unchanged");
    assert_eq!((&row[6], &row[7]), (&Value::Text("key:admin".into()), &Value::Integer(100)));
    assert_eq!(s.pointer().unwrap(), Some(Pointer { name: LIVE.into(), hash: sha(&bundle(1)) }), "the pointer on it");
    assert_eq!(s.versions().unwrap().len(), 2, "versions lists each once");
}

#[test]
fn a_rollback_moves_only_the_pointer() {
    let dir = tmp();
    let path = dir.path().join(UI_DB_FILE);
    let s = UiStore::open(&path).unwrap();
    let a = s.push(version(1, 100)).unwrap();
    let b = s.push(version(2, 200)).unwrap();
    s.set_preview("key:someone", Some(&b.version)).unwrap();
    let (versions, previews) = (rows(&path, "ui_versions"), rows(&path, "ui_preview"));
    assert_eq!(s.rollback(&a.version).unwrap(), a.version);
    assert_eq!(rows(&path, "ui_versions"), versions, "every ui_versions row byte-identical");
    assert_eq!(rows(&path, "ui_preview"), previews, "no preview moved");
    let live = rows(&path, "ui_pointer").into_iter().find(|r| r[0] == Value::Text(LIVE.into()));
    assert_eq!(live, Some(vec![Value::Text(LIVE.into()), Value::Text(a.version.clone())]), "the live pointer moved");
    assert!(matches!(s.rollback(&"0".repeat(64)), Err(UiError::UnknownVersion(_))), "a version not kept is refused");
    assert_eq!(s.pointer().unwrap().map(|p| p.hash), Some(a.version), "and the pointer stays");
}

#[test]
fn an_aborted_push_leaves_no_dangling_pointer() {
    let dir = tmp();
    let path = dir.path().join(UI_DB_FILE);
    let s = UiStore::open(&path).unwrap();
    let a = s.push(version(1, 100)).unwrap();
    let (versions, pointer) = (rows(&path, "ui_versions"), rows(&path, "ui_pointer"));
    let new_hash = sha(&bundle(2));
    let mut seen = None;
    let failed = s.push_then(version(2, 200), |tx| {
        seen = Some(tx.query_row("SELECT count(*) FROM ui_versions WHERE hash = ?1", [&new_hash], |r| r.get::<_, i64>(0))?);
        Err(rusqlite::Error::QueryReturnedNoRows)
    });
    assert_eq!(seen, Some(1), "the hook ran after the insert: the new row visible inside the transaction");
    assert!(failed.is_err(), "the push failed");
    assert_eq!(rows(&path, "ui_versions"), versions, "no new row");
    assert_eq!(rows(&path, "ui_pointer"), pointer, "the pointer where it was");
    assert_eq!(s.pointer().unwrap().map(|p| p.hash), Some(a.version.clone()));
    // The file itself refuses a pointer at a hash it does not store.
    let c = Connection::open(&path).unwrap();
    c.pragma_update(None, "foreign_keys", true).unwrap();
    assert!(c.execute("UPDATE ui_pointer SET hash = ?1 WHERE name = ?2", rusqlite::params!["f".repeat(64), LIVE]).is_err(), "a foreign key");
    assert!(c.execute("INSERT INTO ui_preview (principal, hash) VALUES ('key:x', ?1)", [&"f".repeat(64)]).is_err(), "a preview too");
    let stored: Vec<Value> = rows(&path, "ui_versions").into_iter().map(|r| r[0].clone()).collect();
    for p in rows(&path, "ui_pointer") {
        assert!(stored.contains(&p[1]), "the pointer names a stored hash: {p:?}");
    }
}

#[test]
fn versions_are_newest_first_and_say_which_is_live() {
    let dir = tmp();
    let s = UiStore::open(&dir.path().join(UI_DB_FILE)).unwrap();
    let pushed: Vec<Pushed> = (1..=3).map(|n| s.push(version(n, 100 * n as i64)).unwrap()).collect();
    let listed = s.versions().unwrap();
    assert_eq!(listed.len(), 3, "three rows");
    let hashes: Vec<&str> = listed.iter().map(|(v, _)| v.hash.as_str()).collect();
    assert_eq!(hashes, vec![pushed[2].version.as_str(), pushed[1].version.as_str(), pushed[0].version.as_str()], "newest first");
    assert_eq!(listed.iter().filter(|(_, live)| *live).count(), 1, "exactly one live");
    assert!(listed[0].1, "the newest push is live");
    let (newest, _) = &listed[0];
    assert_eq!(
        (newest.commit.as_deref(), newest.contract.as_str(), newest.abi.as_str(), newest.pushed_by.as_str(), newest.pushed_at),
        (Some("0000003"), "c3", "a3", "key:admin", 300),
        "commit, contract and abi off the manifest, who and when"
    );
    // Same second, still newest first: the order a push came in.
    let tie = s.push(version(4, 300)).unwrap();
    assert_eq!(s.versions().unwrap()[0].0.hash, tie.version);
}

#[test]
fn a_preview_is_per_principal() {
    let dir = tmp();
    let s = UiStore::open(&dir.path().join(UI_DB_FILE)).unwrap();
    let a = s.push(version(1, 100)).unwrap();
    let b = s.push(version(2, 200)).unwrap();
    assert_eq!(s.set_preview("key:alice", Some(&a.version)).unwrap(), Some(a.version.clone()));
    assert_eq!(s.preview_of("key:alice").unwrap(), Some(a.version.clone()), "answered for it");
    assert_eq!(s.preview_of("key:bob").unwrap(), None, "and for nobody else");
    assert_eq!(s.entry_for(Some("key:alice")).unwrap().map(|(h, _)| h), Some(a.version.clone()), "its entrypoint is the preview's");
    assert_eq!(s.entry_for(Some("key:bob")).unwrap().map(|(h, _)| h), Some(b.version.clone()), "everyone else's is live");
    assert_eq!(s.entry_for(None).unwrap().map(|(h, _)| h), Some(b.version.clone()));
    assert_eq!(s.pointer().unwrap().map(|p| p.hash), Some(b.version), "a preview moves no pointer");
    assert_eq!(s.set_preview("key:alice", None).unwrap(), None, "cleared");
    assert_eq!(s.preview_of("key:alice").unwrap(), None);
}

#[test]
fn no_file_means_no_versions() {
    let dir = tmp();
    let path = dir.path().join("nowhere").join(UI_DB_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    assert!(!path.exists());
    let s = UiStore::open(&path).unwrap();
    assert!(s.versions().unwrap().is_empty(), "versions answers none");
    assert_eq!(s.pointer().unwrap(), None, "the pointer is none");
    assert_eq!(s.entry_for(None).unwrap(), None, "and no entrypoint");
}
