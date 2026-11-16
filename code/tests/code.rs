//! CODE-2: the code service against a local server standing in for GitHub
//! (both of its addresses: `/<owner>/<name>.git/info/refs` and
//! `/<owner>/<name>/tar.gz/<commit>`), let through by [`GitHub::at`] — what
//! the hidden `--code-github-at` flag sets — with synthetic repositories
//! built here: a pkt-line listing and gzip tars carrying the pax header
//! GitHub writes. Nothing here reaches the internet. One test per scenario
//! of CODE-2, and the refusals around them.

use menger_code::allowance::{Allowance, Denied};
use menger_code::github::pkt_line;
use menger_code::{Caps, CodeEvent, CodeService, CodeStore, GitHub};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const REPO: &str = "acme/kiln";
const ME: &str = "key:me";
const T0: i64 = 1_790_000_000;

// ── the stand-in ─────────────────────────────────────────────────────────

/// One answer: status, headers, body; `delay` before it.
#[derive(Clone)]
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
}

fn ok(mime: &str, body: Vec<u8>) -> Answer {
    Answer { status: 200, headers: vec![("content-type".into(), mime.into())], body, delay: Duration::ZERO }
}

fn status(s: u16) -> Answer {
    Answer { status: s, headers: vec![("content-type".into(), "text/plain".into())], body: b"no".to_vec(), delay: Duration::ZERO }
}

/// A repository as the stand-in serves it: its listing, and an archive per
/// commit (by the full hash and by every prefix of seven or more).
#[derive(Clone, Default)]
struct Repo {
    listing: Option<Vec<u8>>,
    archives: HashMap<String, Vec<u8>>,
}

/// The stand-in: what it serves, and every path it was asked.
#[derive(Clone)]
struct StandIn {
    at: SocketAddr,
    asked: Arc<Mutex<Vec<String>>>,
}

impl StandIn {
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
    fn listings(&self) -> usize {
        self.asked().iter().filter(|p| p.contains("/info/refs")).count()
    }
    fn archives(&self) -> usize {
        self.asked().iter().filter(|p| p.contains("/tar.gz/")).count()
    }
}

/// A local server answering each path by `route`.
async fn serve(route: impl Fn(&str) -> Answer + Send + Sync + 'static) -> StandIn {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let route = Arc::new(route);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let (route, log) = (route.clone(), log.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                // Nothing that names a caller, a ticket or the tracker goes with a request.
                let lower = head.to_ascii_lowercase();
                assert!(!lower.contains("authorization") && !lower.contains("cookie") && !lower.contains("key:") && !lower.contains("menger.sh"), "{head}");
                log.lock().unwrap().push(path.clone());
                let a = route(&path);
                tokio::time::sleep(a.delay).await;
                let mut out = format!("HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n", a.status, a.body.len());
                for (k, v) in &a.headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.write_all(&a.body).await;
            });
        }
    });
    StandIn { at, asked }
}

/// The stand-in for `repos` (by `owner/name`); anything else 404, a
/// listing GitHub does not show 401 (as GitHub answers it).
async fn github(repos: HashMap<String, Repo>) -> StandIn {
    let repos = Arc::new(Mutex::new(repos));
    serve(move |path| {
        let repos = repos.lock().unwrap();
        let path = path.trim_start_matches('/');
        if let Some(name) = path.strip_suffix(".git/info/refs?service=git-upload-pack") {
            return match repos.get(name).and_then(|r| r.listing.clone()) {
                Some(l) => ok("application/x-git-upload-pack-advertisement", l),
                None => status(401),
            };
        }
        if let Some((name, commit)) = path.split_once("/tar.gz/") {
            let found = repos.get(name).and_then(|r| r.archives.iter().find(|(h, _)| h.starts_with(commit) && commit.len() >= 7).map(|(_, a)| a.clone()));
            return match found {
                Some(a) => ok("application/x-gzip", a),
                None => status(404),
            };
        }
        status(404)
    })
    .await
}

// ── synthetic repositories ───────────────────────────────────────────────

/// A listing: HEAD at `head`'s branch, every branch, every tag (an
/// annotated one with its `^{}` line when it peels).
fn listing(head: &str, branches: &[(&str, &str)], tags: &[(&str, &str, Option<&str>)]) -> Vec<u8> {
    let head_hash = branches.iter().find(|(b, _)| *b == head).map(|(_, h)| *h).unwrap_or("0000000000000000000000000000000000000000");
    let mut t = pkt_line("# service=git-upload-pack\n") + "0000";
    t += &pkt_line(&format!("{head_hash} HEAD\0multi_ack thin-pack side-band symref=HEAD:refs/heads/{head} object-format=sha1 agent=git/stand-in\n"));
    for (b, h) in branches {
        t += &pkt_line(&format!("{h} refs/heads/{b}\n"));
    }
    for (name, h, peeled) in tags {
        t += &pkt_line(&format!("{h} refs/tags/{name}\n"));
        if let Some(p) = peeled {
            t += &pkt_line(&format!("{p} refs/tags/{name}^{{}}\n"));
        }
    }
    (t + "0000").into_bytes()
}

/// A hash of 40 hex digits made of `c`.
fn h(c: char) -> String {
    c.to_string().repeat(40)
}

/// One entry of a synthetic archive.
enum E<'a> {
    File(&'a str, Vec<u8>),
    Dir(&'a str),
    Link(&'a str, &'a str),
    /// A path written into the header as it is, unchecked.
    Raw(&'a str, Vec<u8>),
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let body = format!(" {key}={value}\n");
    let mut n = body.len() + 1;
    while format!("{n}{body}").len() != n {
        n += 1;
    }
    format!("{n}{body}").into_bytes()
}

/// GitHub's archive of `commit`: a pax global header `comment=<commit>`
/// (none when `comment` is false), then every entry under `<top>/`, each
/// with the commit's time.
fn archive_of(commit: &str, comment: bool, entries: &[E]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let top = format!("kiln-{}", &commit[..7]);
    if comment {
        let rec = pax_record("comment", commit);
        let mut hd = tar::Header::new_ustar();
        hd.set_path("pax_global_header").unwrap();
        hd.set_entry_type(tar::EntryType::XGlobalHeader);
        hd.set_size(rec.len() as u64);
        hd.set_mode(0o666);
        hd.set_cksum();
        tar.append(&hd, rec.as_slice()).unwrap();
    }
    let mut dir = tar::Header::new_ustar();
    dir.set_path(format!("{top}/")).unwrap();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_size(0);
    dir.set_mtime(T0 as u64);
    dir.set_mode(0o775);
    dir.set_cksum();
    tar.append(&dir, std::io::empty()).unwrap();
    for e in entries {
        let mut hd = tar::Header::new_ustar();
        hd.set_mtime(T0 as u64);
        hd.set_mode(0o664);
        match e {
            E::File(p, bytes) => {
                hd.set_entry_type(tar::EntryType::Regular);
                hd.set_size(bytes.len() as u64);
                tar.append_data(&mut hd, format!("{top}/{p}"), bytes.as_slice()).unwrap();
            }
            E::Dir(p) => {
                hd.set_entry_type(tar::EntryType::Directory);
                hd.set_size(0);
                tar.append_data(&mut hd, format!("{top}/{p}/"), std::io::empty()).unwrap();
            }
            E::Link(p, to) => {
                hd.set_entry_type(tar::EntryType::Symlink);
                hd.set_size(0);
                tar.append_link(&mut hd, format!("{top}/{p}"), to).unwrap();
            }
            E::Raw(p, bytes) => {
                hd.set_entry_type(tar::EntryType::Regular);
                hd.set_size(bytes.len() as u64);
                let name = &mut hd.as_old_mut().name;
                name[..p.len()].copy_from_slice(p.as_bytes());
                hd.set_cksum();
                tar.append(&hd, bytes.as_slice()).unwrap();
            }
        }
    }
    let raw = tar.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &raw).unwrap();
    gz.finish().unwrap()
}

/// A real 2×1 PNG.
fn png() -> Vec<u8> {
    let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x01\x08\x06\0\0\0".to_vec();
    p.extend_from_slice(&[0xf4, 0x22, 0x7f, 0x8a, 0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);
    p
}

/// `n` bytes of text no other call makes alike: `seed` repeated, numbered.
fn text(seed: &str, n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 64);
    let mut i = 0;
    while out.len() < n {
        out.extend_from_slice(format!("{seed} {i}\n").as_bytes());
        i += 1;
    }
    out.truncate(n);
    out
}

/// The kiln repository: `main` at A (`v1` an annotated tag on A, `light` a
/// plain tag on B), `next` at B — B is A with `src/main.rs` changed — and
/// the files: a README, a nested source, a PNG, a link, a submodule.
fn kiln() -> (Repo, String, String) {
    let (a, b) = (format!("9c41d07e{}", "1".repeat(32)), format!("5be0aa31{}", "2".repeat(32)));
    let common = |main: &[u8]| {
        vec![
            E::File(".gitmodules", b"[submodule \"vendor/glaze\"]\n\tpath = vendor/glaze\n\turl = https://github.com/acme/glaze\n".to_vec()),
            E::File("README.md", b"# kiln\n\nfires pots\n".to_vec()),
            E::Dir("src"),
            E::File("src/main.rs", main.to_vec()),
            E::File("src/lib.rs", b"pub fn fire() {}\n".to_vec()),
            E::Dir("assets"),
            E::File("assets/logo.png", png()),
            E::Link("docs", "README.md"),
            E::Dir("vendor"),
            E::Dir("vendor/glaze"),
        ]
    };
    let mut repo = Repo {
        listing: Some(listing("main", &[("main", &a), ("next", &b)], &[("v1", &h('7'), Some(&a)), ("light", &b, None)])),
        archives: HashMap::new(),
    };
    repo.archives.insert(a.clone(), archive_of(&a, true, &common(b"fn main() {}\n")));
    repo.archives.insert(b.clone(), archive_of(&b, true, &common(b"fn main() { kiln::fire(); }\n")));
    (repo, a, b)
}

// ── the service under test ───────────────────────────────────────────────

/// An allowance that keeps each principal's usage, capped at `cap`.
#[derive(Debug, Default)]
struct Ledger {
    cap: u64,
    used: Mutex<HashMap<String, u64>>,
    holds: Mutex<HashMap<String, (String, u64)>>,
}

impl Ledger {
    fn capped(cap: u64) -> Arc<Self> {
        Arc::new(Self { cap, ..Default::default() })
    }
    fn used(&self, p: &str) -> u64 {
        self.used.lock().unwrap().get(p).copied().unwrap_or(0)
    }
    fn open_holds(&self) -> usize {
        self.holds.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl Allowance for Ledger {
    async fn reserve(&self, principal: &str, amount: u64) -> Result<String, Denied> {
        let held: u64 = self.holds.lock().unwrap().values().filter(|(p, _)| p == principal).map(|(_, n)| n).sum();
        if self.used(principal) + held + amount > self.cap {
            return Err(Denied { reason: "over_allowance".into(), message: format!("over your allowance of code.bytes: {} used of {}", self.used(principal), self.cap), remaining: Some(self.cap - self.used(principal)) });
        }
        let id = format!("hold-{}", self.holds.lock().unwrap().len() + self.used.lock().unwrap().len() * 1000);
        self.holds.lock().unwrap().insert(id.clone(), (principal.into(), amount));
        Ok(id)
    }
    async fn commit(&self, id: &str, amount: u64) {
        if let Some((p, _)) = self.holds.lock().unwrap().remove(id) {
            *self.used.lock().unwrap().entry(p).or_default() += amount;
        }
    }
    async fn release(&self, id: &str) {
        self.holds.lock().unwrap().remove(id);
    }
    async fn credit(&self, principal: &str, amount: u64) {
        let mut used = self.used.lock().unwrap();
        let u = used.entry(principal.into()).or_default();
        *u = u.saturating_sub(amount);
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    code: Arc<CodeService>,
    clock: Arc<AtomicI64>,
    ledger: Arc<Ledger>,
    store: Arc<CodeStore>,
}

fn store_in(dir: &std::path::Path) -> Arc<CodeStore> {
    Arc::new(CodeStore::open(&dir.join("code.db"), &dir.join("code-blobs")).unwrap())
}

/// The service over a fresh store, GitHub at `at`, `ledger`, `max` bytes and `caps`.
fn fixture_with(at: SocketAddr, ledger: Arc<Ledger>, max: u64, caps: Caps) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let clock = Arc::new(AtomicI64::new(T0));
    let c = clock.clone();
    let code = CodeService::new(store.clone())
        .with_github(GitHub::at(at))
        .with_allowance(ledger.clone())
        .with_max_bytes(max)
        .with_caps(caps)
        .with_clock(Arc::new(move || c.load(Ordering::SeqCst)));
    Fixture { _dir: dir, code: Arc::new(code), clock, ledger, store }
}

fn fixture(at: SocketAddr) -> Fixture {
    fixture_with(at, Ledger::capped(u64::MAX / 4), menger_code::DEFAULT_MAX_BYTES, Caps::default())
}

fn kiln_github() -> HashMap<String, Repo> {
    HashMap::from([(REPO.to_string(), kiln().0)])
}

fn reason(ev: &CodeEvent) -> &str {
    match ev {
        CodeEvent::Refused { reason, .. } => reason,
        _ => panic!("not a refusal: {ev:?}"),
    }
}

// ── resolve ──────────────────────────────────────────────────────────────

/// Scenario "a branch": `ref: main` answers its full hash, `tip: true`; a
/// tag its commit (an annotated one, the commit it points at), no tip; none
/// the default branch; a ref the listing lacks `no_such_ref`.
#[tokio::test]
async fn a_branch_resolves_to_its_full_hash_as_a_tip() {
    let (_, a, b) = kiln();
    let gh = github(kiln_github()).await;
    let f = fixture(gh.at);
    let opened = |ev: CodeEvent| match ev {
        CodeEvent::Opened { commit, reference, tip, kept, files, bytes, committed_unix, .. } => (commit, reference, tip, kept, files, bytes, committed_unix),
        ev => panic!("{ev:?}"),
    };
    assert_eq!(opened(f.code.open(REPO, Some("main"), None).await), (a.clone(), Some("main".into()), true, false, None, None, None));
    assert_eq!(opened(f.code.open(REPO, Some("refs/heads/next"), None).await).0, b);
    assert_eq!(opened(f.code.open(REPO, None, None).await), (a.clone(), Some("main".into()), true, false, None, None, None), "the default branch");
    let (c, r, tip, ..) = opened(f.code.open(REPO, Some("v1"), None).await);
    assert_eq!((c, r.as_deref(), tip), (a.clone(), Some("v1"), false), "an annotated tag: the commit it points at");
    assert_eq!(opened(f.code.open(REPO, Some("light"), None).await).0, b);
    assert_eq!(reason(&f.code.open(REPO, Some("nope"), None).await), "no_such_ref");
    // The refs: every branch and tag, and the default.
    let CodeEvent::Refs { default_branch, branches, tags, .. } = f.code.refs(REPO).await else { panic!() };
    assert_eq!((default_branch.as_deref(), branches.len(), tags.len()), (Some("main"), 2, 2));
    assert_eq!(tags.iter().find(|t| t.name == "v1").map(|t| t.commit.clone()), Some(a));
    assert_eq!(gh.listings(), 1, "one listing served all of them");
}

/// Scenario "asked twice in a minute": the stand-in saw one request; a
/// minute later, a second.
#[tokio::test]
async fn a_ref_opened_twice_in_a_minute_asks_github_once() {
    let gh = github(kiln_github()).await;
    let f = fixture(gh.at);
    f.code.open(REPO, Some("main"), None).await;
    f.clock.fetch_add(59, Ordering::SeqCst);
    f.code.open(REPO, Some("main"), None).await;
    assert_eq!(gh.listings(), 1, "{:?}", gh.asked());
    f.clock.fetch_add(2, Ordering::SeqCst);
    f.code.open(REPO, Some("main"), None).await;
    assert_eq!(gh.listings(), 2, "past the minute, asked again");
}

/// Scenario "a short hash that is kept": the full hash and `kept: true`,
/// with no request.
#[tokio::test]
async fn a_kept_short_hash_is_answered_in_full_with_no_request() {
    let (_, a, _) = kiln();
    let gh = github(kiln_github()).await;
    let f = fixture(gh.at);
    assert!(matches!(f.code.fetch(ME, REPO, &a).await, CodeEvent::Fetched { .. }));
    let before = gh.asked().len();
    let ev = f.code.open(REPO, None, Some(&a[..8])).await;
    let CodeEvent::Opened { commit, kept: true, tip: false, files: Some(files), committed_unix: Some(t), .. } = ev else { panic!("{ev:?}") };
    assert_eq!((commit, files, t), (a, 5, T0));
    assert_eq!(gh.asked().len(), before, "nothing asked of GitHub");
}

/// A repository GitHub does not show (its listing 401, as GitHub answers a
/// private or missing one) is `not_public` — to open, and to fetch, whose
/// 404 the listing tells from a missing commit's.
#[tokio::test]
async fn a_repository_github_does_not_show_is_not_public() {
    let (_, a, _) = kiln();
    let gh = github(kiln_github()).await;
    let f = fixture(gh.at);
    assert_eq!(reason(&f.code.open("acme/secret", Some("main"), None).await), "not_public");
    assert_eq!(reason(&f.code.fetch(ME, "acme/secret", &a).await), "not_public");
    assert_eq!(reason(&f.code.fetch(ME, REPO, &h('d')).await), "no_such_commit");
    assert!(f.store.kept().unwrap().is_empty());
}

/// A limit is `rate_limited`, with GitHub's wait.
#[tokio::test]
async fn a_limit_is_said_with_when_to_try_again() {
    let gh = serve(|_| Answer { status: 429, headers: vec![("retry-after".into(), "30".into())], body: Vec::new(), delay: Duration::ZERO }).await;
    let f = fixture(gh.at);
    let ev = f.code.open(REPO, Some("main"), None).await;
    assert!(matches!(&ev, CodeEvent::Refused { reason, retry_after_secs: Some(30), .. } if reason == "rate_limited"), "{ev:?}");
    f.code.open(REPO, Some("main"), None).await;
    assert_eq!(gh.listings(), 2, "a limit is not remembered as an answer");
}

// ── fetch ────────────────────────────────────────────────────────────────

/// A fetch keeps the commit: its files by hash, its tree; a short hash is
/// read in full from the archive's header; a second fetch makes no request.
#[tokio::test]
async fn a_commit_is_fetched_once_and_kept_by_content() {
    let (_, a, _) = kiln();
    let gh = github(kiln_github()).await;
    let f = fixture(gh.at);
    let ev = f.code.fetch(ME, REPO, &a[..10]).await;
    let CodeEvent::Fetched { commit, files, bytes, new_bytes, .. } = ev else { panic!("{ev:?}") };
    assert_eq!((commit.as_str(), files), (a.as_str(), 5));
    assert_eq!(new_bytes, bytes, "every byte new");
    assert_eq!(f.ledger.used(ME), new_bytes, "reserved for the fetcher, and committed");
    assert_eq!(f.store.blobs_on_disk(), 5);
    let again = f.code.fetch(ME, REPO, &a).await;
    assert!(matches!(again, CodeEvent::Fetched { new_bytes: 0, .. }), "{again:?}");
    assert_eq!(gh.archives(), 1, "{:?}", gh.asked());
    let CodeEvent::Kept { fetched_by, .. } = &f.code.kept()[0] else { panic!() };
    assert_eq!(fetched_by.as_deref(), Some(ME));
}

/// Scenario "the next commit of a branch": `new_bytes` is the one changed
/// file's size.
#[tokio::test]
async fn the_next_commit_costs_only_the_file_that_changed() {
    let (_, a, b) = kiln();
    let gh = github(kiln_github()).await;
    let f = fixture(gh.at);
    f.code.fetch(ME, REPO, &a).await;
    let ev = f.code.fetch(ME, REPO, &b).await;
    let CodeEvent::Fetched { new_bytes, files, .. } = ev else { panic!("{ev:?}") };
    assert_eq!((new_bytes, files), ("fn main() { kiln::fire(); }\n".len() as u64, 5));
    assert_eq!(f.store.blobs_on_disk(), 6);
}

/// Scenario "over the allowance": refused as the quota service says it,
/// and nothing kept.
#[tokio::test]
async fn a_fetch_past_the_allowance_is_refused_and_nothing_is_kept() {
    let (_, a, _) = kiln();
    let gh = github(kiln_github()).await;
    let f = fixture_with(gh.at, Ledger::capped(10), menger_code::DEFAULT_MAX_BYTES, Caps::default());
    let ev = f.code.fetch(ME, REPO, &a).await;
    assert_eq!(reason(&ev), "over_allowance", "{ev:?}");
    assert!(f.store.kept().unwrap().is_empty() && f.store.blobs_on_disk() == 0 && f.store.total().unwrap() == 0);
    assert_eq!((f.ledger.used(ME), f.ledger.open_holds()), (0, 0), "the hold given back");
    assert!(matches!(f.code.tree(REPO, &a, ""), CodeEvent::NotKept { .. }));
}

/// Scenario "a path that leaves the tree": refused, nothing kept — `..`,
/// an absolute path, and one outside the top directory.
#[tokio::test]
async fn an_archive_whose_path_leaves_the_tree_is_refused_whole() {
    let bad = [
        (h('a'), "../../etc/passwd"),
        (h('b'), "kiln-bbbbbbb/../../etc/passwd"),
        (h('c'), "/etc/passwd"),
        (h('e'), "elsewhere/x"),
    ];
    let mut repo = Repo { listing: Some(listing("main", &[("main", &h('a'))], &[])), archives: HashMap::new() };
    for (c, p) in &bad {
        repo.archives.insert(c.clone(), archive_of(c, true, &[E::File("ok.txt", b"fine\n".to_vec()), E::Raw(p, b"root:x:0:0\n".to_vec())]));
    }
    let gh = github(HashMap::from([(REPO.to_string(), repo)])).await;
    let f = fixture(gh.at);
    for (c, p) in &bad {
        let ev = f.code.fetch(ME, REPO, c).await;
        assert_eq!(reason(&ev), "bad_path", "{p}: {ev:?}");
    }
    assert!(f.store.kept().unwrap().is_empty() && f.store.blobs_on_disk() == 0, "nothing kept");
    assert_eq!(f.ledger.used(ME), 0);
    let staging = std::fs::read_dir(f.store.staging()).unwrap().count();
    assert_eq!(staging, 0, "nothing staged is left");
}

/// The caps: past 400 MiB unpacked, 50,000 entries or 100 MiB compressed
/// (here, small ones) refused `too_large`, naming the cap; nothing kept.
#[tokio::test]
async fn an_archive_past_a_cap_is_refused_naming_it() {
    let (_, a, _) = kiln();
    let gh = github(kiln_github()).await;
    for (caps, max) in [
        (Caps { unpacked: 64, ..Caps::default() }, 64),
        (Caps { entries: 4, ..Caps::default() }, 4),
        (Caps { compressed: 100, ..Caps::default() }, 100),
    ] {
        let f = fixture_with(gh.at, Ledger::capped(u64::MAX / 4), menger_code::DEFAULT_MAX_BYTES, caps);
        let ev = f.code.fetch(ME, REPO, &a).await;
        assert!(matches!(&ev, CodeEvent::Refused { reason, max: Some(m), .. } if reason == "too_large" && *m == max), "{caps:?}: {ev:?}");
        assert!(f.store.kept().unwrap().is_empty() && f.store.blobs_on_disk() == 0);
    }
}

/// Two fetches of one commit at once make one request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_fetches_at_once_make_one_request() {
    let (mut repo, a, _) = kiln();
    let archive = repo.archives.remove(&a).unwrap();
    let a2 = a.clone();
    let gh = serve(move |p| match p.contains("/tar.gz/") && p.ends_with(&a2) {
        true => Answer { delay: Duration::from_millis(300), ..ok("application/x-gzip", archive.clone()) },
        false => status(404),
    })
    .await;
    let f = fixture(gh.at);
    let (one, two) = tokio::join!(f.code.fetch(ME, REPO, &a), f.code.fetch("key:you", REPO, &a));
    let news: Vec<u64> = [one, two].iter().map(|e| match e {
        CodeEvent::Fetched { new_bytes, .. } => *new_bytes,
        e => panic!("{e:?}"),
    }).collect();
    assert_eq!(gh.archives(), 1, "{:?}", gh.asked());
    assert!(news.contains(&0) && news.iter().any(|n| *n > 0), "{news:?}");
}

// ── read ─────────────────────────────────────────────────────────────────

/// Scenario "GitHub is unreachable": a kept commit's tree and a file are
/// answered with no route to GitHub. And what `tree` and `read` answer:
/// entries ordered by path with their kinds, a directory's, text whole,
/// a link's target, not_found, not_kept.
#[tokio::test]
async fn the_tree_and_a_file_are_answered_from_what_is_kept() {
    let (_, a, _) = kiln();
    let gh = github(kiln_github()).await;
    let first = fixture(gh.at);
    first.code.fetch(ME, REPO, &a).await;
    // The same store, GitHub nowhere: a port nothing listens on.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let code = CodeService::new(first.store.clone()).with_github(GitHub::at(dead));
    let ev = code.tree(REPO, &a[..7], "");
    let CodeEvent::Tree { commit, entries, truncated: false, .. } = ev else { panic!("{ev:?}") };
    assert_eq!(commit, a);
    let seen: Vec<(&str, &str)> = entries.iter().map(|e| (e.path.as_str(), e.kind.as_str())).collect();
    assert_eq!(
        seen,
        [(".gitmodules", "file"), ("README.md", "file"), ("assets", "dir"), ("assets/logo.png", "file"), ("docs", "link"), ("src", "dir"), ("src/lib.rs", "file"), ("src/main.rs", "file"), ("vendor", "dir"), ("vendor/glaze", "module")]
    );
    assert_eq!(entries.iter().find(|e| e.path == "docs").and_then(|e| e.target.clone()).as_deref(), Some("README.md"));
    let CodeEvent::Tree { entries, .. } = code.tree(REPO, &a, "src") else { panic!() };
    assert_eq!(entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["src/lib.rs", "src/main.rs"]);
    assert_eq!(code.read(REPO, &a, "src/main.rs"), CodeEvent::File { path: "src/main.rs".into(), bytes: 13, lines: 1, text: "fn main() {}\n".into() });
    assert_eq!(code.read(REPO, &a, "README.md"), CodeEvent::File { path: "README.md".into(), bytes: 19, lines: 3, text: "# kiln\n\nfires pots\n".into() });
    assert_eq!(code.read(REPO, &a, "docs"), CodeEvent::Link { path: "docs".into(), target: "README.md".into() });
    assert_eq!(code.read(REPO, &a, "nope.rs"), CodeEvent::NotFound { path: "nope.rs".into() });
    assert_eq!(code.read(REPO, &a, "src"), CodeEvent::NotFound { path: "src".into() });
    assert_eq!(code.tree(REPO, &a, "README.md"), CodeEvent::NotFound { path: "README.md".into() });
    assert_eq!(code.read(REPO, &h('f'), "README.md"), CodeEvent::NotKept { repo: REPO.into(), commit: h('f') });
    assert_eq!(code.read("acme/other", &a, "README.md"), CodeEvent::NotKept { repo: "acme/other".into(), commit: a.clone() });
    // GitHub was never asked by any of them.
    assert_eq!(gh.archives(), 1);
}

/// Scenario "a binary file": a PNG answers `binary` with its size and no
/// text; text past 2 MiB `too_large`; a file that is not UTF-8 `binary`.
#[tokio::test]
async fn a_binary_or_large_file_is_said_without_its_bytes() {
    let c = h('3');
    let big = text("line", 2 * 1024 * 1024 + 1);
    let mut latin = b"caf".to_vec();
    latin.push(0xe9);
    let repo = Repo {
        listing: None,
        archives: HashMap::from([(c.clone(), archive_of(&c, true, &[E::File("logo.png", png()), E::File("big.txt", big), E::File("latin1.txt", latin)]))]),
    };
    let gh = github(HashMap::from([(REPO.to_string(), repo)])).await;
    let f = fixture(gh.at);
    assert!(matches!(f.code.fetch(ME, REPO, &c).await, CodeEvent::Fetched { files: 3, .. }));
    assert_eq!(f.code.read(REPO, &c, "logo.png"), CodeEvent::Binary { path: "logo.png".into(), bytes: png().len() as u64 });
    assert_eq!(f.code.read(REPO, &c, "big.txt"), CodeEvent::TooLarge { path: "big.txt".into(), bytes: 2 * 1024 * 1024 + 1, max: 2 * 1024 * 1024 });
    assert_eq!(f.code.read(REPO, &c, "latin1.txt"), CodeEvent::Binary { path: "latin1.txt".into(), bytes: 4 });
}

// ── kept ─────────────────────────────────────────────────────────────────

/// A commit of `n` bytes in one file of its own, built from `seed`.
fn sized(seed: char, n: usize) -> (String, Vec<u8>) {
    let c = h(seed);
    let archive = archive_of(&c, true, &[E::File("blob.txt", text(&seed.to_string(), n))]);
    (c, archive)
}

/// Scenario "the cap": at 1 MiB, two commits of 600 KiB with no file in
/// common fetched an hour apart, the first not read since: the first is
/// gone, the second kept, and the first fetcher's usage back where it was.
/// One read within the hour keeps a commit; past that, `full`.
#[tokio::test]
async fn the_store_stays_under_its_cap_dropping_the_commit_read_longest_ago() {
    let ((c1, a1), (c2, a2), (c3, a3)) = (sized('4', 600 * 1024), sized('5', 600 * 1024), sized('6', 600 * 1024));
    let repo = Repo { listing: None, archives: HashMap::from([(c1.clone(), a1), (c2.clone(), a2), (c3.clone(), a3)]) };
    let gh = github(HashMap::from([(REPO.to_string(), repo)])).await;
    let f = fixture_with(gh.at, Ledger::capped(u64::MAX / 4), 1024 * 1024, Caps::default());
    assert!(matches!(f.code.fetch("key:first", REPO, &c1).await, CodeEvent::Fetched { new_bytes: 614_400, .. }));
    assert_eq!(f.ledger.used("key:first"), 614_400);
    f.clock.fetch_add(3600, Ordering::SeqCst);
    let ev = f.code.fetch("key:second", REPO, &c2).await;
    assert!(matches!(ev, CodeEvent::Fetched { new_bytes: 614_400, .. }), "{ev:?}");
    assert!(f.store.commit(REPO, &c1).unwrap().is_none(), "the first is gone");
    assert!(f.store.commit(REPO, &c2).unwrap().is_some(), "the second kept");
    assert_eq!((f.ledger.used("key:first"), f.ledger.used("key:second")), (0, 614_400), "the first fetcher's usage back where it was");
    assert_eq!((f.store.blobs_on_disk(), f.store.total().unwrap()), (1, 614_400));
    // The second was read within the hour: a third does not fit — `full`, nothing dropped.
    f.clock.fetch_add(1800, Ordering::SeqCst);
    assert!(matches!(f.code.tree(REPO, &c2, ""), CodeEvent::Tree { .. }));
    f.clock.fetch_add(1800, Ordering::SeqCst);
    let ev = f.code.fetch("key:third", REPO, &c3).await;
    assert_eq!(reason(&ev), "full", "{ev:?}");
    assert!(f.store.commit(REPO, &c2).unwrap().is_some() && f.store.commit(REPO, &c3).unwrap().is_none());
    assert_eq!((f.ledger.used("key:third"), f.ledger.open_holds()), (0, 0));
    assert_eq!(f.store.blobs_on_disk(), 1);
}

/// `kept` lists each commit; `forget` drops one, crediting its fetcher —
/// a file another kept commit names stays.
#[tokio::test]
async fn kept_lists_each_commit_and_forget_credits_its_bytes() {
    let (_, a, b) = kiln();
    let gh = github(kiln_github()).await;
    let f = fixture(gh.at);
    let CodeEvent::Fetched { bytes, .. } = f.code.fetch("key:first", REPO, &a).await else { panic!() };
    f.code.fetch("key:second", REPO, &b).await;
    let kept = f.code.kept();
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert!(matches!(kept.last(), Some(CodeEvent::Store { commits: 2, max_bytes, .. }) if *max_bytes == menger_code::DEFAULT_MAX_BYTES));
    let ev = f.code.forget(REPO, &a[..7]).await;
    let only_a = "fn main() {}\n".len() as u64;
    assert_eq!(ev, CodeEvent::Forgotten { repo: REPO.into(), commit: a.clone(), freed: only_a }, "the files b names stay");
    assert_eq!(f.ledger.used("key:first"), bytes - only_a);
    assert_eq!(f.code.forget(REPO, &a).await, CodeEvent::NotKept { repo: REPO.into(), commit: a.clone() });
    let ev = f.code.forget(REPO, &b).await;
    assert!(matches!(ev, CodeEvent::Forgotten { .. }));
    assert_eq!((f.store.blobs_on_disk(), f.ledger.used("key:first"), f.ledger.used("key:second")), (0, 0, 0));
}

/// An archive with no pax comment is kept when its full hash was asked,
/// and refused when only a short one was (its full hash unknown).
#[tokio::test]
async fn an_archive_without_its_hash_is_kept_only_by_a_full_hash() {
    let c = h('8');
    let repo = Repo { listing: None, archives: HashMap::from([(c.clone(), archive_of(&c, false, &[E::File("a.txt", b"a\n".to_vec())]))]) };
    let gh = github(HashMap::from([(REPO.to_string(), repo)])).await;
    let f = fixture(gh.at);
    assert_eq!(reason(&f.code.fetch(ME, REPO, &c[..9]).await), "bad_archive");
    assert!(matches!(f.code.fetch(ME, REPO, &c).await, CodeEvent::Fetched { files: 1, .. }));
}
