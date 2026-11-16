//! **What each method does**, beside the activation (its `impl` keeps only
//! the methods — MENGER-29's rule). [`CodeService`] holds the store, the way to
//! GitHub, the allowance, the cap and the clock; a test builds one over a
//! stand-in and a clock of its own.
//!
//! - `open` asks the listing at most once a minute per repository, and
//!   nothing at all for a commit that is kept;
//! - `fetch` asks the archive once per commit: two fetches of one commit at
//!   once wait on one another, the second finding it kept; the bytes new to
//!   the store are reserved for the caller before anything is kept, and the
//!   keeping (the cap, what goes for room) is one at a time;
//! - `tree` and `read` answer from the store alone and mark the commit read.

use crate::allowance::{Allowance, Unlimited};
use crate::events::{CodeEvent, RefAt};
use crate::github::{GitHub, Listing};
use crate::store::{CodeStore, CommitRow, StoreError};
use crate::unpack::{Caps, Stage, Unpacked};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The time, in unix seconds — a test's may be set.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// How long a listing is reused: a minute.
pub const LISTING_SECS: i64 = 60;
/// The most entries one `tree` answers.
pub const TREE_MAX: usize = 20_000;
/// The largest file `read` answers as text: 2 MiB.
pub const READ_MAX: u64 = 2 * 1024 * 1024;
/// How much of a file is looked at for a NUL: 8 KiB.
pub const SNIFF: usize = 8 * 1024;

fn system_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

type Listed = Result<Arc<Listing>, CodeEvent>;

/// The service: what is kept, and how GitHub is read.
pub struct CodeService {
    store: Arc<CodeStore>,
    github: GitHub,
    allowance: Arc<dyn Allowance>,
    max_bytes: u64,
    caps: Caps,
    clock: Clock,
    listings: Mutex<HashMap<String, (i64, Listed)>>,
    fetching: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    keeping: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for CodeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeService").field("store", &self.store.path()).field("github", &self.github).field("max_bytes", &self.max_bytes).finish()
    }
}

/// A store error as the wire says it.
fn said(e: StoreError) -> CodeEvent {
    match e {
        StoreError::Ambiguous(_) => CodeEvent::refused("ambiguous", e.to_string()),
        StoreError::Full { max, .. } => CodeEvent::Refused { reason: "full".into(), message: format!("{e} (nothing was kept)"), retry_after_secs: None, max: Some(max) },
        e => CodeEvent::Error { message: e.to_string() },
    }
}

fn fetched(row: &CommitRow, new_bytes: u64) -> CodeEvent {
    CodeEvent::Fetched { repo: row.repo.clone(), commit: row.hash.clone(), files: row.files, bytes: row.bytes, new_bytes }
}

impl CodeService {
    /// Over `store`: GitHub itself, every byte allowed, the default cap and
    /// caps, the system's clock.
    pub fn new(store: Arc<CodeStore>) -> Self {
        Self {
            store,
            github: GitHub::new(),
            allowance: Arc::new(Unlimited),
            max_bytes: crate::DEFAULT_MAX_BYTES,
            caps: Caps::default(),
            clock: Arc::new(system_now),
            listings: Mutex::new(HashMap::new()),
            fetching: Mutex::new(HashMap::new()),
            keeping: tokio::sync::Mutex::new(()),
        }
    }

    /// GitHub read at `github` (a test's stand-in).
    pub fn with_github(mut self, github: GitHub) -> Self {
        self.github = github;
        self
    }

    /// Every new byte asked of `allowance` (the quota service's meter).
    pub fn with_allowance(mut self, allowance: Arc<dyn Allowance>) -> Self {
        self.allowance = allowance;
        self
    }

    /// The kept bytes held under `max` (`--code-max-bytes`).
    pub fn with_max_bytes(mut self, max: u64) -> Self {
        self.max_bytes = max;
        self
    }

    /// An archive held to `caps` (a test's small ones).
    pub fn with_caps(mut self, caps: Caps) -> Self {
        self.caps = caps;
        self
    }

    /// The time read from `clock` (a test's).
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// The store.
    pub fn store(&self) -> &Arc<CodeStore> {
        &self.store
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// `repo`'s listing: the one asked within the last minute, else asked
    /// now. A repository not there is remembered as long; a failure to
    /// reach GitHub is not.
    pub async fn listing(&self, repo: &str) -> Listed {
        let now = self.now();
        if let Some((at, listed)) = self.listings.lock().unwrap_or_else(|p| p.into_inner()).get(repo) {
            if now - at < LISTING_SECS {
                return listed.clone();
            }
        }
        let listed = self.github.list(repo).await.map(Arc::new);
        let keep = !matches!(&listed, Err(CodeEvent::Refused { reason, .. }) if reason == "unreachable" || reason == "rate_limited");
        if keep {
            self.listings.lock().unwrap_or_else(|p| p.into_inner()).insert(repo.to_string(), (now, listed.clone()));
        }
        listed
    }

    fn opened(&self, repo: &str, commit: &str, reference: Option<String>, tip: bool) -> CodeEvent {
        match self.store.commit(repo, commit) {
            Ok(row) => CodeEvent::Opened {
                repo: repo.into(),
                commit: commit.into(),
                reference,
                tip,
                committed_unix: row.as_ref().and_then(|r| r.committed_unix),
                kept: row.is_some(),
                files: row.as_ref().map(|r| r.files),
                bytes: row.as_ref().map(|r| r.bytes),
            },
            Err(e) => said(e),
        }
    }

    /// `code.open`: the commit `reference` (a branch, a tag; the default
    /// branch when none) or `commit` names, and whether it is kept.
    pub async fn open(&self, repo: &str, reference: Option<&str>, commit: Option<&str>) -> CodeEvent {
        if let Some(c) = commit {
            match self.store.resolve(repo, c) {
                Ok(Some(full)) => return self.opened(repo, &full, None, false),
                Err(e) => return said(e),
                Ok(None) if c.len() == 40 => return self.opened(repo, c, None, false),
                Ok(None) => {}
            }
            let listing = match self.listing(repo).await {
                Ok(l) => l,
                Err(e) => return e,
            };
            let full = listing.branches.values().chain(listing.tags.values()).find(|h| h.starts_with(c)).cloned().unwrap_or_else(|| c.to_string());
            return self.opened(repo, &full, None, false);
        }
        let listing = match self.listing(repo).await {
            Ok(l) => l,
            Err(e) => return e,
        };
        match resolve(&listing, reference) {
            Some((name, hash, tip)) => self.opened(repo, &hash, Some(name), tip),
            None => match reference {
                Some(r) => CodeEvent::refused("no_such_ref", format!("{repo} has no branch or tag `{r}` on GitHub")),
                None => CodeEvent::refused("no_such_ref", format!("{repo} has no default branch on GitHub (an empty repository?)")),
            },
        }
    }

    /// `code.refs`: the listing's branches and tags, and the default branch.
    pub async fn refs(&self, repo: &str) -> CodeEvent {
        match self.listing(repo).await {
            Ok(l) => {
                let at = |m: &std::collections::BTreeMap<String, String>| m.iter().map(|(n, c)| RefAt { name: n.clone(), commit: c.clone() }).collect();
                CodeEvent::Refs { repo: repo.into(), default_branch: l.head.clone(), branches: at(&l.branches), tags: at(&l.tags) }
            }
            Err(e) => e,
        }
    }

    fn kept_already(&self, repo: &str, commit: &str) -> Option<CodeEvent> {
        match self.store.resolve(repo, commit) {
            Ok(Some(full)) => match self.store.commit(repo, &full) {
                Ok(Some(row)) => Some(fetched(&row, 0)),
                Ok(None) => None,
                Err(e) => Some(said(e)),
            },
            Ok(None) => None,
            Err(e) => Some(said(e)),
        }
    }

    /// `code.fetch`: `commit` of `repo` read from GitHub once and kept, for `by`.
    pub async fn fetch(&self, by: &str, repo: &str, commit: &str) -> CodeEvent {
        if let Some(ev) = self.kept_already(repo, commit) {
            return ev;
        }
        let key = format!("{repo}@{commit}");
        let lock = self.fetching.lock().unwrap_or_else(|p| p.into_inner()).entry(key.clone()).or_default().clone();
        let out = {
            let _one = lock.lock().await;
            self.fetch_one(by, repo, commit).await
        };
        let mut fetching = self.fetching.lock().unwrap_or_else(|p| p.into_inner());
        if Arc::strong_count(&lock) <= 2 {
            fetching.remove(&key);
        }
        out
    }

    async fn fetch_one(&self, by: &str, repo: &str, commit: &str) -> CodeEvent {
        if let Some(ev) = self.kept_already(repo, commit) {
            return ev;
        }
        let stage = match Stage::new(&self.store.staging()) {
            Ok(s) => s,
            Err(e) => return CodeEvent::Error { message: format!("the code store's staging: {e}") },
        };
        let archive = stage.path().join("archive.tar.gz");
        if let Err(ev) = self.github.archive(repo, commit, &archive, self.caps.compressed).await {
            // GitHub's 404 is the commit's or the repository's: the listing says which.
            return match &ev {
                CodeEvent::Refused { reason, .. } if reason == "no_such_commit" => self.listing(repo).await.err().unwrap_or(ev),
                _ => ev,
            };
        }
        let (caps, dir) = (self.caps, stage.path().to_path_buf());
        let staged = match tokio::task::spawn_blocking(move || crate::unpack::unpack(&archive, &dir, &caps)).await {
            Ok(Ok(s)) => s,
            Ok(Err(u)) => return unpacked(repo, u),
            Err(e) => return CodeEvent::Error { message: format!("reading the archive: {e}") },
        };
        let full = match staged.commit.clone() {
            Some(h) if h.len() == 40 && h.starts_with(commit) => h,
            None if commit.len() == 40 => commit.to_string(),
            other => return CodeEvent::refused("bad_archive", format!("{repo}: the archive asked for {commit} names {} (nothing was kept)", other.as_deref().unwrap_or("no commit"))),
        };
        self.keep(by, repo, &full, staged, &stage).await
    }

    async fn keep(&self, by: &str, repo: &str, full: &str, staged: crate::unpack::Staged, stage: &Stage) -> CodeEvent {
        let _one = self.keeping.lock().await;
        match self.store.commit(repo, full) {
            Ok(Some(row)) => return fetched(&row, 0),
            Ok(None) => {}
            Err(e) => return said(e),
        }
        let new = match self.store.new_bytes(&staged) {
            Ok(n) => n,
            Err(e) => return said(e),
        };
        let hold = match new {
            0 => None,
            n => match self.allowance.reserve(by, n).await {
                Ok(id) => Some(id),
                Err(d) => return CodeEvent::Refused { reason: d.reason, message: format!("{} (nothing was kept)", d.message), retry_after_secs: None, max: d.remaining },
            },
        };
        let (store, dir, now, max) = (self.store.clone(), stage.path().to_path_buf(), self.now(), self.max_bytes);
        let (r, h, b) = (repo.to_string(), full.to_string(), by.to_string());
        let kept = tokio::task::spawn_blocking(move || store.keep(&r, &h, &staged, &dir, &b, now, max)).await;
        let kept = match kept {
            Ok(Ok(k)) => k,
            Ok(Err(e)) => {
                self.settle(hold, None).await;
                return said(e);
            }
            Err(e) => {
                self.settle(hold, None).await;
                return CodeEvent::Error { message: format!("keeping the commit: {e}") };
            }
        };
        self.settle(hold, Some(kept.new_bytes)).await;
        for (whose, bytes) in &kept.credits {
            self.allowance.credit(whose, *bytes).await;
        }
        for (r, h) in &kept.dropped {
            tracing::info!("code:      {r}@{h} dropped for room (read longest ago)");
        }
        CodeEvent::Fetched { repo: repo.into(), commit: full.into(), files: kept.files, bytes: kept.bytes, new_bytes: kept.new_bytes }
    }

    async fn settle(&self, hold: Option<String>, kept: Option<u64>) {
        match (hold, kept) {
            (Some(id), Some(n)) => self.allowance.commit(&id, n).await,
            (Some(id), None) => self.allowance.release(&id).await,
            (None, _) => {}
        }
    }

    /// The kept commit `commit` begins, or what says it is not.
    fn kept_commit(&self, repo: &str, commit: &str) -> Result<String, CodeEvent> {
        match self.store.resolve(repo, commit) {
            Ok(Some(full)) => Ok(full),
            Ok(None) => Err(CodeEvent::NotKept { repo: repo.into(), commit: commit.into() }),
            Err(e) => Err(said(e)),
        }
    }

    /// `code.tree`: every entry under `path`, from the store alone.
    pub fn tree(&self, repo: &str, commit: &str, path: &str) -> CodeEvent {
        let full = match self.kept_commit(repo, commit) {
            Ok(h) => h,
            Err(ev) => return ev,
        };
        let _ = self.store.touch(repo, &full, self.now());
        match self.store.tree(repo, &full, path, TREE_MAX) {
            Ok(Some((entries, truncated))) => CodeEvent::Tree { repo: repo.into(), commit: full, path: path.into(), entries, truncated },
            Ok(None) => CodeEvent::NotFound { path: path.into() },
            Err(e) => said(e),
        }
    }

    /// `code.read`: a file of a kept commit, from the store alone.
    pub fn read(&self, repo: &str, commit: &str, path: &str) -> CodeEvent {
        let full = match self.kept_commit(repo, commit) {
            Ok(h) => h,
            Err(ev) => return ev,
        };
        let _ = self.store.touch(repo, &full, self.now());
        let row = match self.store.entry(repo, &full, path) {
            Ok(Some(r)) => r,
            Ok(None) => return CodeEvent::NotFound { path: path.into() },
            Err(e) => return said(e),
        };
        match (row.kind.as_str(), row.blob, row.target) {
            ("file", _, _) if row.bytes > READ_MAX => CodeEvent::TooLarge { path: path.into(), bytes: row.bytes, max: READ_MAX },
            ("file", Some(blob), _) => match self.store.read_blob(&blob) {
                Ok(bytes) if bytes[..bytes.len().min(SNIFF)].contains(&0) => CodeEvent::Binary { path: path.into(), bytes: row.bytes },
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => CodeEvent::File { path: path.into(), bytes: row.bytes, lines: text.lines().count() as u64, text },
                    Err(_) => CodeEvent::Binary { path: path.into(), bytes: row.bytes },
                },
                Err(e) => said(e),
            },
            ("link", _, Some(target)) => CodeEvent::Link { path: path.into(), target },
            _ => CodeEvent::NotFound { path: path.into() },
        }
    }

    /// `code.kept`: each kept commit, then the store as a whole.
    pub fn kept(&self) -> Vec<CodeEvent> {
        let rows = match self.store.kept() {
            Ok(r) => r,
            Err(e) => return vec![said(e)],
        };
        let total = self.store.total().unwrap_or(0);
        let mut out: Vec<CodeEvent> = rows
            .iter()
            .map(|r| CodeEvent::Kept {
                repo: r.repo.clone(),
                commit: r.hash.clone(),
                files: r.files,
                bytes: r.bytes,
                committed_unix: r.committed_unix,
                fetched_by: r.fetched_by.clone(),
                fetched_unix: r.fetched_unix,
                read_unix: r.read_unix,
            })
            .collect();
        out.push(CodeEvent::Store { commits: rows.len() as u64, bytes: total, max_bytes: self.max_bytes });
        out
    }

    /// `code.forget`: the kept commit `commit` begins dropped, its bytes
    /// credited to whom they were reserved for.
    pub async fn forget(&self, repo: &str, commit: &str) -> CodeEvent {
        let _one = self.keeping.lock().await;
        let full = match self.kept_commit(repo, commit) {
            Ok(h) => h,
            Err(ev) => return ev,
        };
        let (store, r, h) = (self.store.clone(), repo.to_string(), full.clone());
        match tokio::task::spawn_blocking(move || store.forget(&r, &h)).await {
            Ok(Ok(Some((freed, credits)))) => {
                for (whose, bytes) in &credits {
                    self.allowance.credit(whose, *bytes).await;
                }
                CodeEvent::Forgotten { repo: repo.into(), commit: full, freed }
            }
            Ok(Ok(None)) => CodeEvent::NotKept { repo: repo.into(), commit: commit.into() },
            Ok(Err(e)) => said(e),
            Err(e) => CodeEvent::Error { message: format!("forgetting the commit: {e}") },
        }
    }
}

/// An archive's refusal as the wire says it.
fn unpacked(repo: &str, u: Unpacked) -> CodeEvent {
    let max = match &u {
        Unpacked::TooLarge { max, .. } => Some(*max),
        _ => None,
    };
    let reason = match &u {
        Unpacked::TooLarge { .. } => "too_large",
        Unpacked::BadPath(_) => "bad_path",
        Unpacked::Bad(_) => "bad_archive",
    };
    CodeEvent::Refused { reason: reason.into(), message: format!("{repo}: {u} (nothing was kept)"), retry_after_secs: None, max }
}

/// The ref `reference` names in `listing` (the default branch when none):
/// its name, its commit, and whether it is a branch's tip.
fn resolve(listing: &Listing, reference: Option<&str>) -> Option<(String, String, bool)> {
    let branch = |b: &str| listing.branches.get(b).map(|h| (b.to_string(), h.clone(), true));
    let tag = |t: &str| listing.tags.get(t).map(|h| (t.to_string(), h.clone(), false));
    match reference {
        None => listing.head.as_deref().and_then(branch),
        Some(r) => match (r.strip_prefix("refs/heads/"), r.strip_prefix("refs/tags/")) {
            (Some(b), _) => branch(b),
            (_, Some(t)) => tag(t),
            _ => branch(r).or_else(|| tag(r)),
        },
    }
}
