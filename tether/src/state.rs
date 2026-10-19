//! TETHER-9: what tether keeps across restarts, in `<data-dir>/state.json`
//! — its sessions (each with the inputs it asked and the prompts waiting
//! for its next turn) and the listener's position on the remote (since
//! when it listens, the last comment it read, the answers and replies it
//! has acted on). One file, written whole to a temporary beside it and
//! renamed over it, 0600; a tail is not kept (it is substrate's, in arbor).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::sessions::Session;

/// A session as kept: itself and what waits for its next turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeptSession {
    pub session: Session,
    #[serde(default)]
    pub pending: Vec<String>,
}

/// Where the listener stands on the remote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// The remote it was listening to: another remote starts over.
    #[serde(default)]
    pub remote: Option<String>,
    /// Unix seconds: answers given before this are not tether's to act on
    /// (set at its first listen, so a backlog is never replayed).
    #[serde(default)]
    pub since: Option<i64>,
    /// The newest comment read (comment ids only grow).
    #[serde(default)]
    pub comment_mark: Option<u64>,
    /// What it has acted on — `a:<ticket>:<input>:<answered at>` and
    /// `c:<comment id>` — oldest first, at most [`HANDLED_MAX`].
    #[serde(default)]
    pub handled: Vec<String>,
}

/// The most handled keys kept; past it the oldest go, and `since` moves
/// up to the oldest answer kept, so nothing dropped can come back.
pub const HANDLED_MAX: usize = 5_000;

impl Position {
    pub fn has(&self, key: &str) -> bool {
        self.handled.iter().any(|k| k == key)
    }

    pub fn mark(&mut self, key: String) {
        if self.has(&key) {
            return;
        }
        self.handled.push(key);
        if self.handled.len() > HANDLED_MAX {
            let gone: Vec<String> = self.handled.drain(..self.handled.len() - HANDLED_MAX).collect();
            let newest_gone = gone.iter().filter_map(|k| k.strip_prefix("a:")).filter_map(|k| k.rsplit(':').next()?.parse::<i64>().ok()).max();
            if let Some(at) = newest_gone {
                self.since = Some(self.since.unwrap_or(at).max(at + 1));
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(default)]
    pub sessions: Vec<KeptSession>,
    #[serde(default)]
    pub listen: Position,
}

/// The file, and what was last written to it.
pub struct Store {
    path: PathBuf,
    kept: Mutex<Kept>,
}

impl Store {
    /// Read `<dir>/state.json` (none yet: empty). A file that does not
    /// read is refused rather than overwritten.
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("state.json");
        let kept = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e} (move it aside to start over)", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Kept::default(),
            Err(e) => anyhow::bail!("{}: {e}", path.display()),
        };
        Ok(Self { path, kept: Mutex::new(kept) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn kept(&self) -> Kept {
        self.lock().clone()
    }

    /// Change what is kept and write it.
    pub fn update<R>(&self, f: impl FnOnce(&mut Kept) -> R) -> R {
        let mut kept = self.lock();
        let r = f(&mut kept);
        if let Err(e) = write(&self.path, &kept) {
            tracing::error!("cannot keep tether's state in {}: {e}", self.path.display());
        }
        r
    }
}

fn write(path: &Path, kept: &Kept) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(serde_json::to_string_pretty(kept).map_err(std::io::Error::other)?.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_is_read_back_and_a_bad_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        s.update(|k| {
            k.listen.since = Some(10);
            k.listen.mark("c:4".into());
        });
        let again = Store::open(dir.path()).unwrap();
        assert_eq!(again.kept().listen.since, Some(10));
        assert!(again.kept().listen.has("c:4"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(s.path()).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::write(s.path(), "{not json").unwrap();
        assert!(Store::open(dir.path()).is_err());
    }

    #[test]
    fn past_the_cap_the_oldest_go_and_since_moves_past_them() {
        let mut p = Position { since: Some(0), ..Default::default() };
        for i in 0..HANDLED_MAX + 2 {
            p.mark(format!("a:T-1:x:{i}"));
        }
        assert_eq!(p.handled.len(), HANDLED_MAX);
        assert_eq!(p.since, Some(2), "answers 0 and 1 were dropped: none before 2 is acted on again");
    }
}
