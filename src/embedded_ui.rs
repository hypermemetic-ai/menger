//! DIST-19: **the UI the binary carries**, and making it live at start.
//!
//! A menger installed on a new machine has no bundle to push and no signer
//! to push it with, so its `/app/` was the placeholder. A build that finds a
//! bundle (`build.rs`: `embed/ui.tar`, or `MENGER_UI_BUNDLE`) includes it —
//! [`BUNDLE`] — and `main` hands it to [`seed`] once the ui file is open,
//! before anything is served.
//!
//! The rule is whose move the live pointer last was:
//!
//! - nothing is live: the bundle is kept and made live, by [`SEEDER`];
//! - the last move was this binary's kind (an older binary's seed): the
//!   bundle replaces it — an upgrade brings its UI;
//! - anyone else moved it last (a push, a rollback, an author, a promote,
//!   an undo): nothing is written. A UI somebody chose stands.
//!
//! The bundle is kept unsigned — the binary is the trust, as it is for the
//! code beside it — through the store alone; `ui.push` still wants a
//! `--ui-signer`'s signature.

use plexus_ui_service::store::{files_of_bundle, hash_of};
use plexus_ui_service::{Keep, NewVersion, UiError, UiStore};

/// Who a seeded version is pushed by, and who its pointer move is by.
pub const SEEDER: &str = "menger:embedded";

/// The bundle this binary carries: the tar `plexus-ui build` wrote.
#[cfg(menger_embedded_ui)]
pub const BUNDLE: Option<&[u8]> = Some(include_bytes!(env!("MENGER_EMBEDDED_UI")));
/// The bundle this binary carries: none (the build found no bundle).
#[cfg(not(menger_embedded_ui))]
pub const BUNDLE: Option<&[u8]> = None;

/// What [`seed`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seeded {
    /// The bundle was made live now; `replaced`: the seed it took over from.
    Live { version: String, replaced: Option<String> },
    /// The bundle is what was live already.
    Already { version: String },
    /// Another version is live by somebody's move (`by`), and stands.
    Stands { version: String, by: String },
}

impl std::fmt::Display for Seeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let short = |v: &str| v.chars().take(12).collect::<String>();
        match self {
            Seeded::Live { version, replaced: None } => write!(f, "the UI this binary carries is live ({})", short(version)),
            Seeded::Live { version, replaced: Some(old) } => write!(f, "the UI this binary carries is live ({}, after {})", short(version), short(old)),
            Seeded::Already { version } => write!(f, "the UI this binary carries is live already ({})", short(version)),
            Seeded::Stands { version, by } => write!(f, "the live UI ({}) was put there by {by}: the one this binary carries is left out", short(version)),
        }
    }
}

/// Makes `bundle` live on `store`'s channel when nothing is, or when the
/// live version is an earlier seed; leaves any other live version alone.
pub fn seed(store: &UiStore, bundle: &[u8]) -> Result<Seeded, UiError> {
    let version = hash_of(bundle);
    let mut replaced = None;
    if let Some(live) = store.pointer()? {
        if live.hash == version {
            return Ok(Seeded::Already { version });
        }
        // A pointer with no move kept (a file from before moves were) is
        // somebody's too.
        let by = store.history(1)?.into_iter().next().map_or_else(|| "a move nobody recorded".to_string(), |m| m.by);
        if by != SEEDER {
            return Ok(Seeded::Stands { version: live.hash, by });
        }
        replaced = Some(live.hash);
    }
    let manifest = files_of_bundle(bundle)
        .into_iter()
        .find(|(path, _)| path == "manifest.json")
        .and_then(|(_, bytes)| String::from_utf8(bytes).ok())
        .ok_or_else(|| UiError::BadBundle("the bundle this binary carries holds no manifest.json".into()))?;
    let commit = serde_json::from_str::<serde_json::Value>(&manifest).ok().and_then(|m| m["commit"].as_str().map(str::to_string));
    let pushed_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let v = NewVersion { bundle: bundle.to_vec(), manifest, commit, pushed_by: SEEDER.to_string(), pushed_at };
    let pushed = store.push_kept(v, &version, &Keep::default())?;
    Ok(Seeded::Live { version: pushed.version, replaced })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle the store takes: `index.html` saying `page`, and a manifest.
    fn bundle(page: &str) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (path, body) in [("index.html", page), ("manifest.json", r#"{"abi":"engine/1","contract":"unknown","commit":"menger@abc1234"}"#)] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, path, body.as_bytes()).unwrap();
        }
        b.into_inner().unwrap()
    }

    fn store() -> (tempfile::TempDir, UiStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = UiStore::open(&dir.path().join("ui.db")).unwrap();
        (dir, store)
    }

    fn live(store: &UiStore) -> Option<String> {
        store.pointer().unwrap().map(|p| p.hash)
    }

    #[test]
    fn nothing_live_and_the_bundle_is_made_live_by_the_seeder() {
        let (_dir, store) = store();
        let one = bundle("<p>one</p>");
        let seeded = seed(&store, &one).unwrap();
        assert_eq!(seeded, Seeded::Live { version: hash_of(&one), replaced: None });
        assert_eq!(live(&store), Some(hash_of(&one)));
        let (row, is_live) = store.versions().unwrap().into_iter().next().unwrap();
        assert!(is_live);
        assert_eq!((row.pushed_by.as_str(), row.commit.as_deref(), row.signer), (SEEDER, Some("menger@abc1234"), None));
        assert_eq!(store.file(&row.hash, "index.html").unwrap().as_deref(), Some(&b"<p>one</p>"[..]));
        // A second start of the same binary writes nothing.
        assert_eq!(seed(&store, &one).unwrap(), Seeded::Already { version: hash_of(&one) });
        assert_eq!(store.history(10).unwrap().len(), 1);
    }

    #[test]
    fn an_upgraded_binary_replaces_the_seed_before_it() {
        let (_dir, store) = store();
        let (one, two) = (bundle("<p>one</p>"), bundle("<p>two</p>"));
        seed(&store, &one).unwrap();
        assert_eq!(seed(&store, &two).unwrap(), Seeded::Live { version: hash_of(&two), replaced: Some(hash_of(&one)) });
        assert_eq!(live(&store), Some(hash_of(&two)));
    }

    #[test]
    fn a_version_somebody_pushed_stands() {
        let (_dir, store) = store();
        let (one, theirs) = (bundle("<p>one</p>"), bundle("<p>theirs</p>"));
        seed(&store, &one).unwrap();
        let v = NewVersion { bundle: theirs.clone(), manifest: r#"{"abi":"engine/1","contract":"unknown"}"#.into(), commit: None, pushed_by: "key:abc".into(), pushed_at: 1 };
        store.push(v).unwrap();
        let two = bundle("<p>two</p>");
        assert_eq!(seed(&store, &two).unwrap(), Seeded::Stands { version: hash_of(&theirs), by: "key:abc".into() });
        assert_eq!(live(&store), Some(hash_of(&theirs)));
        assert_eq!(store.versions().unwrap().len(), 2, "the bundle left out is not kept either");
    }

    #[test]
    fn a_rollback_onto_a_seed_is_somebodys_move_and_stands() {
        let (_dir, store) = store();
        let (one, two) = (bundle("<p>one</p>"), bundle("<p>two</p>"));
        seed(&store, &one).unwrap();
        seed(&store, &two).unwrap();
        store.rollback_by(&hash_of(&one), "key:admin").unwrap();
        // The binary carrying `two` starts again: the admin's choice stands.
        assert_eq!(seed(&store, &two).unwrap(), Seeded::Stands { version: hash_of(&one), by: "key:admin".into() });
        assert_eq!(live(&store), Some(hash_of(&one)));
    }

    #[test]
    fn a_bundle_with_no_manifest_is_refused_and_nothing_is_live() {
        let (_dir, store) = store();
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(1);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, "index.html", &b"x"[..]).unwrap();
        let err = seed(&store, &b.into_inner().unwrap()).unwrap_err();
        assert!(err.to_string().contains("manifest.json"), "{err}");
        assert_eq!(live(&store), None);
    }
}
