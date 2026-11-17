//! **Reading GitHub** — the two addresses, and nothing else, through the
//! media service's [`Remote`] and every rule of it (public addresses only,
//! the connection pinned, a time limit, a byte cap), each fetch held to its
//! own host so a redirect anywhere else is refused:
//!
//! - a ref's commit: `https://github.com/<owner>/<name>.git/info/refs?service=git-upload-pack`,
//!   git's own listing (not the REST API, and outside its hourly limit);
//! - a commit's tree: `https://codeload.github.com/<owner>/<name>/tar.gz/<commit>`.
//!
//! No credential, no cookie, and nothing that names the caller, a ticket or
//! the tracker goes with either: the `User-Agent` is the product's
//! ([`AGENT`]). What GitHub answers, checked by hand (CODE-2, 2026-11-15):
//! the listing is protocol v0 — a `# service=git-upload-pack` line, a
//! flush, then `<hash> HEAD\0<capabilities>` (among them
//! `symref=HEAD:refs/heads/<default>`) and `<hash> <ref>` lines, an
//! annotated tag followed by its `^{}` line, the commit it points at; a
//! repository that is not there, or private, answers **401** (with
//! `WWW-Authenticate: Basic`), never 404. The archive takes a full or a
//! short hash, has no `Content-Length`, and begins with a pax global header
//! `comment=<full hash>`; a repository not there or private, and a commit
//! not there, answer 404.

use crate::events::CodeEvent;
use menger_media::remote::{Limits, Refusal, Remote};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

/// The `User-Agent` both reads send: the product, never a person or a place.
pub const AGENT: &str = "menger-code/1";

/// The time one listing may take.
pub const LISTING_TIMEOUT: Duration = Duration::from_secs(15);
/// The most a listing may be: 16 MiB (a repository of many thousand refs).
pub const LISTING_MAX: usize = 16 * 1024 * 1024;
/// The time one archive may take, whole.
pub const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(300);

/// Where the two reads go, and the fetcher they go through.
#[derive(Debug, Clone)]
pub struct GitHub {
    listing: String,
    archive: String,
    remote: Remote,
}

impl Default for GitHub {
    fn default() -> Self {
        Self::new()
    }
}

/// A repository's listing: its default branch, and every branch and tag
/// with its commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// The default branch's name (`symref=HEAD:refs/heads/<it>`).
    pub head: Option<String>,
    pub branches: BTreeMap<String, String>,
    /// Each tag's commit: an annotated tag's `^{}` line, else its own.
    pub tags: BTreeMap<String, String>,
}

impl GitHub {
    /// GitHub itself: `https://github.com` and `https://codeload.github.com`.
    pub fn new() -> Self {
        Self { listing: "https://github.com".into(), archive: "https://codeload.github.com".into(), remote: Remote::new() }
    }

    /// For a test alone: a local server at `at` standing in for both hosts
    /// (`http://<at>/<owner>/<name>.git/info/refs…` and
    /// `http://<at>/<owner>/<name>/tar.gz/<commit>`), let through although
    /// it is not public ([`Remote::allowing`]); every other rule holds.
    pub fn at(at: SocketAddr) -> Self {
        Self { listing: format!("http://{at}"), archive: format!("http://{at}"), remote: Remote::allowing(vec![at]) }
    }

    /// The listing's address for `repo` (`owner/name`).
    pub fn listing_url(&self, repo: &str) -> String {
        format!("{}/{repo}.git/info/refs?service=git-upload-pack", self.listing)
    }

    /// The archive's address for `commit` of `repo`.
    pub fn archive_url(&self, repo: &str, commit: &str) -> String {
        format!("{}/{repo}/tar.gz/{commit}", self.archive)
    }

    fn limits(base: &str, timeout: Duration, max: usize, accept: Vec<&'static str>) -> Limits {
        let host = url::Url::parse(base).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
        Limits { timeout, max, accept, refuse: vec![], hosts: vec![host], agent: AGENT, accept_header: "*/*" }
    }

    /// `repo`'s listing, asked once.
    pub async fn list(&self, repo: &str) -> Result<Listing, CodeEvent> {
        let limits = Self::limits(&self.listing, LISTING_TIMEOUT, LISTING_MAX, vec![]);
        let got = self.remote.get(&self.listing_url(repo), &limits).await.map_err(|r| refusal(repo, None, r))?;
        parse_listing(&got.bytes).map_err(|why| CodeEvent::refused("bad_listing", format!("{repo}: GitHub's listing could not be read: {why}")))
    }

    /// `commit`'s archive of `repo` written to `to`, at most `max` bytes.
    /// GitHub's 404 is said as `no_such_commit`; the caller asks the listing
    /// whether it is the repository that is not there.
    pub async fn archive(&self, repo: &str, commit: &str, to: &Path, max: u64) -> Result<u64, CodeEvent> {
        use std::io::Write;
        let limits = Self::limits(&self.archive, ARCHIVE_TIMEOUT, max as usize, vec![]);
        let mut file = std::fs::File::create(to).map_err(|e| CodeEvent::Error { message: format!("{}: {e}", to.display()) })?;
        let mut sink = |chunk: &[u8]| file.write_all(chunk).map_err(|e| e.to_string());
        let got = self.remote.stream(&self.archive_url(repo, commit), &limits, &mut sink).await.map_err(|r| refusal(repo, Some(commit), r))?;
        file.flush().map_err(|e| CodeEvent::Error { message: e.to_string() })?;
        Ok(got.bytes)
    }
}

/// A fetch's refusal as the wire says it.
fn refusal(repo: &str, commit: Option<&str>, r: Refusal) -> CodeEvent {
    match (&r, commit) {
        (Refusal::Status(401 | 403 | 404), None) => CodeEvent::refused("not_public", format!("{repo}: GitHub shows no public repository of that name (it answered {}); only public repositories are read", status(&r))),
        (Refusal::Status(401 | 403), Some(_)) => CodeEvent::refused("not_public", format!("{repo}: GitHub shows no public repository of that name")),
        (Refusal::Status(404), Some(c)) => CodeEvent::refused("no_such_commit", format!("{repo} has no commit {c} on GitHub")),
        (Refusal::Limited { retry_after_secs }, _) => CodeEvent::Refused { reason: "rate_limited".into(), message: format!("GitHub: {r}"), retry_after_secs: *retry_after_secs, max: None },
        (Refusal::TooLarge { max }, _) => CodeEvent::Refused { reason: "too_large".into(), message: format!("{repo}: the archive is past {} compressed (nothing was kept)", size(*max as u64)), retry_after_secs: None, max: Some(*max as u64) },
        _ => CodeEvent::refused("unreachable", format!("GitHub could not be read ({}): {r}", r.reason())),
    }
}

fn status(r: &Refusal) -> String {
    match r {
        Refusal::Status(s) => s.to_string(),
        other => other.reason().to_string(),
    }
}

/// `n` bytes as a person reads them, in powers of 1024 (the caps are).
pub fn size(n: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    match n {
        n if n >= MIB && n % MIB == 0 => format!("{} MiB", n / MIB),
        n if n >= MIB => format!("{:.1} MiB", n as f64 / MIB as f64),
        n if n >= 1024 => format!("{} KiB", n / 1024),
        n => format!("{n} B"),
    }
}

/// Reads git's protocol v0 listing: pkt-lines, each four hex digits of
/// length then its payload, `0000` a flush.
pub fn parse_listing(bytes: &[u8]) -> Result<Listing, String> {
    let mut out = Listing::default();
    let mut at = 0;
    while at < bytes.len() {
        let len = bytes.get(at..at + 4).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| usize::from_str_radix(h, 16).ok()).ok_or_else(|| format!("no pkt-line length at byte {at}"))?;
        if len == 0 {
            at += 4;
            continue;
        }
        let line = bytes.get(at + 4..at + len).ok_or_else(|| format!("a pkt-line of {len} bytes at byte {at} runs past the end"))?;
        at += len.max(4);
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end_matches('\n');
        if line.starts_with('#') {
            continue;
        }
        let (line, caps) = line.split_once('\0').unwrap_or((line, ""));
        if let Some(head) = caps.split(' ').find_map(|c| c.strip_prefix("symref=HEAD:refs/heads/")) {
            out.head = Some(head.to_string());
        }
        let Some((hash, name)) = line.split_once(' ') else { continue };
        if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let hash = hash.to_ascii_lowercase();
        if let Some(b) = name.strip_prefix("refs/heads/") {
            out.branches.insert(b.to_string(), hash);
        } else if let Some(t) = name.strip_prefix("refs/tags/") {
            match t.strip_suffix("^{}") {
                Some(peeled) => {
                    out.tags.insert(peeled.to_string(), hash);
                }
                None => {
                    out.tags.entry(t.to_string()).or_insert(hash);
                }
            }
        }
    }
    Ok(out)
}

/// One pkt-line of `payload` (what a test's stand-in writes).
pub fn pkt_line(payload: &str) -> String {
    format!("{:04x}{payload}", payload.len() + 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GitHub's listing as it came on 2026-11-15, cut down: the service
    /// line, a flush, HEAD with the capabilities, a branch, an annotated
    /// tag and its peeled line, a lightweight tag, a pull request's ref.
    #[test]
    fn the_listing_reads_branches_tags_and_the_default_branch() {
        let (h, b, t, p, l) = ("1".repeat(40), "2".repeat(40), "3".repeat(40), "4".repeat(40), "5".repeat(40));
        let mut text = pkt_line("# service=git-upload-pack\n") + "0000";
        text += &pkt_line(&format!("{h} HEAD\0multi_ack thin-pack symref=HEAD:refs/heads/main filter object-format=sha1 agent=git/github\n"));
        text += &pkt_line(&format!("{h} refs/heads/main\n"));
        text += &pkt_line(&format!("{b} refs/heads/CODE-2/x\n"));
        text += &pkt_line(&format!("{t} refs/tags/v1\n"));
        text += &pkt_line(&format!("{p} refs/tags/v1^{{}}\n"));
        text += &pkt_line(&format!("{l} refs/tags/light\n"));
        text += &pkt_line(&format!("{l} refs/pull/1/head\n"));
        text += "0000";
        let got = parse_listing(text.as_bytes()).unwrap();
        assert_eq!(got.head.as_deref(), Some("main"));
        assert_eq!(got.branches.get("main"), Some(&h));
        assert_eq!(got.branches.get("CODE-2/x"), Some(&b));
        assert_eq!(got.tags.get("v1"), Some(&p), "an annotated tag answers the commit it points at");
        assert_eq!(got.tags.get("light"), Some(&l));
        assert_eq!(got.branches.len() + got.tags.len(), 4, "a pull request's ref is neither");
        assert!(parse_listing(b"00zz").is_err());
        assert!(parse_listing(b"0040abc").is_err(), "a line past the end");
    }

    #[test]
    fn sizes_read_as_the_caps_are_written() {
        assert_eq!(size(100 * 1024 * 1024), "100 MiB");
        assert_eq!(size(2048), "2 KiB");
        assert_eq!(size(7), "7 B");
    }
}
