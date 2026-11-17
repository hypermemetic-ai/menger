//! NOTES-4: **the bundle `notes`** — where the components written for notes
//! are kept, apart from the app's own bundle — and the ui service's answer
//! to the notes service's one question about it.
//!
//! **Kept from the first start.** `ui.author { bundle: "notes" }` builds on
//! the bundle's newest version, so one must be kept before the first
//! author. At every start, before anything is served, [`seed`] keeps one
//! where the pointer `bundle:notes` names none: a tar of a manifest alone
//! (`name: "notes"`, no component), kept aside (`live: false`), the
//! bundle's pointer moved onto it — the store's own `--no-live` push, as
//! [`crate::embedded_ui`] keeps the binary's UI, unsigned (the binary is
//! the trust). Its bytes are the same on every menger, so it is one version
//! wherever it is kept. A start that finds the pointer writes nothing: what
//! has been authored into the bundle since stands. A deploy pushes the
//! app's bundle under its own name (or none), which never moves this
//! pointer (FRAME-15), so a deploy leaves the bundle and every version in it
//! as they were; nothing in `deploy/` pushes it.
//!
//! On the box the ui store is one file that test and prod both mount
//! (UI-15, `/shared/ui.db`) and a bundle's pointer is the file's, not a
//! channel's: the first start of a release carrying this — test's — keeps
//! the seed for both, and prod, on the release before, has a bundle `notes`
//! it does not use yet. A component authored on test is kept in the same
//! file and is prod's to draw too; each environment's slots are in its own
//! `notes.db`, so neither fills the other's notes.
//!
//! **Does this version hold this component** ([`UiComponents`]): the ui
//! store's own answer — the version resolved as `ui.rollback` resolves one
//! (a hash, or its first 6 hex digits or more), its manifest's `components`
//! read — through the store the ui service runs on, never its file opened
//! again: plexus-ui 0.2.6 has no method that says what a version holds.

use menger_notes::{Components, Held};
use plexus_ui_service::store::hash_of;
use plexus_ui_service::{How, Keep, ManifestDoc, NewVersion, UiError, UiStore};
use std::sync::Arc;

/// The bundle's name.
pub const BUNDLE: &str = "notes";

/// Who the seed is pushed by.
pub const SEEDER: &str = "menger:notes";

/// The seed's manifest: the bundle's name, no component.
pub const SEED_MANIFEST: &str = "{\n  \"abi\": \"engine/1\",\n  \"name\": \"notes\",\n  \"contract\": \"unknown\",\n  \"routes\": {},\n  \"slots\": {},\n  \"components\": {}\n}\n";

/// The seed: a tar of [`SEED_MANIFEST`] alone, the same bytes every time
/// (no time, no owner in its header).
pub fn seed_bundle() -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_ustar();
    h.set_size(SEED_MANIFEST.len() as u64);
    h.set_mode(0o644);
    h.set_mtime(0);
    h.set_cksum();
    b.append_data(&mut h, "manifest.json", SEED_MANIFEST.as_bytes()).expect("a tar in memory");
    b.into_inner().expect("a tar in memory")
}

/// What [`seed`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seeded {
    /// The seed is kept now, the bundle's pointer on it.
    Kept { version: String },
    /// The bundle was kept already (its newest version).
    Already { version: String },
}

impl std::fmt::Display for Seeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let short = |v: &str| v.chars().take(12).collect::<String>();
        match self {
            Seeded::Kept { version } => write!(f, "the bundle {BUNDLE} is kept ({}): notes' components are authored into it", short(version)),
            Seeded::Already { version } => write!(f, "the bundle {BUNDLE} is kept, its newest {}", short(version)),
        }
    }
}

/// Keeps the bundle `notes` on `store` where none is: see the module.
pub fn seed(store: &UiStore) -> Result<Seeded, UiError> {
    if let Some(version) = store.named(BUNDLE)? {
        return Ok(Seeded::Already { version });
    }
    let bundle = seed_bundle();
    let version = hash_of(&bundle);
    let pushed_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let v = NewVersion { bundle, manifest: SEED_MANIFEST.to_string(), commit: None, pushed_by: SEEDER.to_string(), pushed_at };
    let keep = Keep { live: false, aside_moves_name: true, how: How::Push, ..Keep::default() };
    let pushed = store.push_kept(v, &version, &keep)?;
    Ok(Seeded::Kept { version: pushed.version })
}

/// The ui service's answer to the notes service ([`Components`]).
pub struct UiComponents(pub Arc<UiStore>);

impl Components for UiComponents {
    fn holds(&self, version: &str, component: &str) -> Result<Held, String> {
        let hash = match self.0.resolve(version) {
            Ok(h) => h,
            Err(UiError::UnknownVersion(_)) => return Ok(Held::NoVersion),
            Err(e) => return Err(e.to_string()),
        };
        let manifest = self.0.file(&hash, "manifest.json").map_err(|e| e.to_string())?.unwrap_or_default();
        let doc = ManifestDoc::parse(&String::from_utf8_lossy(&manifest)).map_err(|e| e.to_string())?;
        Ok(match doc.components.contains_key(component) {
            true => Held::Holds(hash),
            false => Held::NoComponent(hash),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, UiStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = UiStore::open(&dir.path().join("ui.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn the_seed_is_kept_once_never_live_the_same_bytes_everywhere() {
        let (_dir, s) = store();
        let kept = seed(&s).unwrap();
        let Seeded::Kept { version } = kept.clone() else { panic!("{kept:?}") };
        assert_eq!(version, hash_of(&seed_bundle()), "the same bytes every time");
        assert_eq!(s.named(BUNDLE).unwrap().as_deref(), Some(version.as_str()));
        assert!(s.pointer().unwrap().is_none(), "never live");
        assert_eq!(seed(&s).unwrap(), Seeded::Already { version: version.clone() }, "a second start writes nothing");
        assert_eq!(s.versions().unwrap().len(), 1);
    }

    #[test]
    fn a_version_is_asked_for_its_component() {
        let (_dir, s) = store();
        let Seeded::Kept { version } = seed(&s).unwrap() else { panic!() };
        let ui = UiComponents(Arc::new(s));
        assert_eq!(ui.holds(&version, "graph-nodes").unwrap(), Held::NoComponent(version.clone()));
        assert_eq!(ui.holds(&version[..8], "graph-nodes").unwrap(), Held::NoComponent(version.clone()), "a prefix is resolved");
        assert_eq!(ui.holds(&"0".repeat(64), "graph-nodes").unwrap(), Held::NoVersion);
    }
}
