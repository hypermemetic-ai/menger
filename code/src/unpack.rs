//! **A commit's archive, read into a staged tree** — GitHub's gzip tar
//! (`codeload.github.com/…/tar.gz/<commit>`), entry by entry, under the
//! caps, before anything is kept:
//!
//! - the full hash from the pax global header (`comment=<hash>`);
//! - every entry under one top directory (`<name>-<commit>/`), dropped;
//!   one outside it, absolute, or holding `..`, refuses the whole archive
//!   (`bad_path`);
//! - each regular file streamed to the staging directory while it is
//!   hashed, and kept there once by its SHA-256 — memory holds one chunk,
//!   never a file;
//! - a symbolic link kept as its target's text, never followed; a hard
//!   link the same, its target in the tree;
//! - the caps (`too_large`, naming the one passed): the files' bytes
//!   ([`Caps::unpacked`]), the entries ([`Caps::entries`]), and the
//!   decompressed stream itself, so headers alone cannot run past them.
//!
//! The entries' time is the commit's (`committed_unix`). A directory that
//! `.gitmodules` names is a submodule (`module`): the archive holds it
//! empty.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// What one archive may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// The archive as it comes: 100 MiB.
    pub compressed: u64,
    /// Its files' bytes, all together: 400 MiB.
    pub unpacked: u64,
    /// Its entries, directories included: 50,000.
    pub entries: u64,
}

impl Default for Caps {
    fn default() -> Self {
        Self { compressed: 100 * 1024 * 1024, unpacked: 400 * 1024 * 1024, entries: 50_000 }
    }
}

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Link,
    Module,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Dir => "dir",
            Kind::Link => "link",
            Kind::Module => "module",
        }
    }
}

/// One entry of the staged tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub kind: Kind,
    /// A file's size; 0 otherwise.
    pub bytes: u64,
    /// A file's SHA-256, its blob's name in the staging directory.
    pub hash: Option<String>,
    /// A link's target.
    pub target: Option<String>,
}

/// An archive read whole, its files in the staging directory by hash.
#[derive(Debug, Clone)]
pub struct Staged {
    /// The pax global header's `comment`: the commit's full hash.
    pub commit: Option<String>,
    pub committed_unix: Option<i64>,
    pub entries: Vec<Entry>,
}

impl Staged {
    /// Its files, and their bytes all together.
    pub fn totals(&self) -> (u64, u64) {
        self.entries.iter().filter(|e| e.kind == Kind::File).fold((0, 0), |(n, b), e| (n + 1, b + e.bytes))
    }
}

/// Why an archive was refused; nothing of it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unpacked {
    /// Past a cap: which, and the cap.
    TooLarge { what: &'static str, max: u64 },
    /// An entry whose path leaves the tree.
    BadPath(String),
    /// Not a gzip tar GitHub writes, in words.
    Bad(String),
}

impl std::fmt::Display for Unpacked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unpacked::TooLarge { what, max } if *what == "entries" => write!(f, "the archive holds more than {max} entries"),
            Unpacked::TooLarge { what, max } => write!(f, "the archive is past {} {what}", crate::github::size(*max)),
            Unpacked::BadPath(p) => write!(f, "the archive holds `{p}`, a path that leaves the tree"),
            Unpacked::Bad(why) => write!(f, "the archive could not be read: {why}"),
        }
    }
}

/// A reader that refuses past `max` bytes.
struct Capped<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n as u64 > self.left {
            return Err(std::io::Error::other(CAPPED));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

const CAPPED: &str = "the decompressed archive passed its cap";

/// Its path, checked: relative, under `top`, no `..`. `Ok(None)`: the top
/// directory itself.
fn relative(raw: &[u8], top: &mut Option<String>) -> Result<Option<String>, Unpacked> {
    let text = std::str::from_utf8(raw).map_err(|_| Unpacked::BadPath(String::from_utf8_lossy(raw).into_owned()))?;
    if text.starts_with('/') || text.contains('\0') {
        return Err(Unpacked::BadPath(text.to_string()));
    }
    let parts: Vec<&str> = text.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.contains(&"..") || parts.is_empty() {
        return Err(Unpacked::BadPath(text.to_string()));
    }
    match top {
        None => *top = Some(parts[0].to_string()),
        Some(t) if t == parts[0] => {}
        Some(_) => return Err(Unpacked::BadPath(text.to_string())),
    }
    Ok((parts.len() > 1).then(|| parts[1..].join("/")))
}

/// Streams one file into `stage`, kept once by its hash: (hash, bytes).
fn stage_file(r: &mut impl Read, stage: &Path, n: u64) -> std::io::Result<(String, u64)> {
    let part = stage.join(format!("{n}.part"));
    let mut out = std::fs::File::create(&part)?;
    let (mut hasher, mut buf, mut bytes) = (Sha256::new(), vec![0u8; 64 * 1024], 0u64);
    loop {
        let got = r.read(&mut buf)?;
        if got == 0 {
            break;
        }
        hasher.update(&buf[..got]);
        out.write_all(&buf[..got])?;
        bytes += got as u64;
    }
    drop(out);
    let hash: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let at = stage.join(&hash);
    match at.exists() {
        true => std::fs::remove_file(&part)?,
        false => std::fs::rename(&part, &at)?,
    }
    Ok((hash, bytes))
}

fn io(e: std::io::Error) -> Unpacked {
    match e.to_string().contains(CAPPED) {
        true => Unpacked::TooLarge { what: "unpacked", max: 0 },
        false => Unpacked::Bad(e.to_string()),
    }
}

/// `archive` (a gzip tar) read into `stage` under `caps`.
pub fn unpack(archive: &Path, stage: &Path, caps: &Caps) -> Result<Staged, Unpacked> {
    let file = std::fs::File::open(archive).map_err(|e| Unpacked::Bad(e.to_string()))?;
    // Headers are 512 bytes, a long name or a pax record a few more: the
    // stream's cap is the files' and room for every entry's headers.
    let left = caps.unpacked.saturating_add(caps.entries.saturating_add(16).saturating_mul(3 * 512));
    let gz = Capped { inner: flate2::read::GzDecoder::new(std::io::BufReader::new(file)), left };
    let mut tar = tar::Archive::new(gz);
    let (mut out, mut top, mut total, mut count) = (Staged { commit: None, committed_unix: None, entries: Vec::new() }, None, 0u64, 0u64);
    let unpacked = |e: Unpacked| match e {
        Unpacked::TooLarge { what: "unpacked", .. } => Unpacked::TooLarge { what: "unpacked", max: caps.unpacked },
        e => e,
    };
    for entry in tar.entries().map_err(io).map_err(unpacked)? {
        let mut entry = entry.map_err(io).map_err(unpacked)?;
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            if let Some(exts) = entry.pax_extensions().map_err(io)? {
                for ext in exts.flatten() {
                    if ext.key() == Ok("comment") {
                        out.commit = ext.value().ok().map(|v| v.trim().to_ascii_lowercase());
                    }
                }
            }
            continue;
        }
        if kind.is_pax_local_extensions() || kind.is_gnu_longname() || kind.is_gnu_longlink() {
            continue;
        }
        count += 1;
        if count > caps.entries {
            return Err(Unpacked::TooLarge { what: "entries", max: caps.entries });
        }
        out.committed_unix = out.committed_unix.or_else(|| entry.header().mtime().ok().map(|t| t as i64));
        let Some(path) = relative(&entry.path_bytes(), &mut top)? else { continue };
        let staged = match kind {
            tar::EntryType::Regular | tar::EntryType::Continuous | tar::EntryType::GNUSparse => {
                total += entry.header().size().unwrap_or(0);
                if total > caps.unpacked {
                    return Err(Unpacked::TooLarge { what: "unpacked", max: caps.unpacked });
                }
                let (hash, bytes) = stage_file(&mut entry, stage, count).map_err(io).map_err(unpacked)?;
                Entry { path, kind: Kind::File, bytes, hash: Some(hash), target: None }
            }
            tar::EntryType::Directory => Entry { path, kind: Kind::Dir, bytes: 0, hash: None, target: None },
            tar::EntryType::Symlink => Entry { target: Some(target_of(&entry)), path, kind: Kind::Link, bytes: 0, hash: None },
            tar::EntryType::Link => {
                let t = target_of(&entry);
                let t = relative(t.as_bytes(), &mut top)?.unwrap_or_default();
                Entry { target: Some(t), path, kind: Kind::Link, bytes: 0, hash: None }
            }
            // Devices and pipes: git writes none; nothing to show.
            _ => continue,
        };
        out.entries.push(staged);
    }
    modules(&mut out, stage);
    out.entries.sort_by(|a, b| a.path.cmp(&b.path));
    out.entries.dedup_by(|a, b| a.path == b.path);
    Ok(out)
}

/// A link's target, as text.
fn target_of<R: Read>(e: &tar::Entry<'_, R>) -> String {
    e.link_name_bytes().map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// The directories `.gitmodules` names are submodules.
fn modules(out: &mut Staged, stage: &Path) {
    let Some(hash) = out.entries.iter().find(|e| e.path == ".gitmodules").and_then(|e| e.hash.clone()) else { return };
    let Ok(text) = std::fs::read_to_string(stage.join(hash)) else { return };
    let paths: Vec<String> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("path"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .map(|p| p.trim().trim_matches('/').to_string())
        .collect();
    for e in out.entries.iter_mut().filter(|e| e.kind == Kind::Dir && paths.contains(&e.path)) {
        e.kind = Kind::Module;
    }
}

/// A staging directory, removed with whatever it holds when dropped.
#[derive(Debug)]
pub struct Stage(pub PathBuf);

impl Stage {
    /// A new one under `root`.
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let dir = root.join(uuid::Uuid::new_v4().simple().to_string());
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
