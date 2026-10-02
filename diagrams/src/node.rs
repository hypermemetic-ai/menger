//! The Node program, as a child process the service owns (COMPILE-4).
//!
//! mermaid is JavaScript; COMPILE-3 showed the vendored bundle gives the
//! browser's verdict under Node with jsdom, and nothing short of that does.
//! So the service keeps one Node process — `diagrams/node/mermaid-parse.js`
//! — and asks it one diagram at a time over stdio: a JSON request per line
//! on its stdin, a JSON answer per line on its stdout (the protocol is
//! documented at the top of that file).
//!
//! How the process is kept, and what each failure becomes:
//!
//! | when | what happens |
//! |---|---|
//! | the first call | the program is started — it loads jsdom and the bundle (~0.5 s) and says `ready` — and kept |
//! | later calls | reuse it, one diagram at a time, in arrival order (a FIFO lock); each request carries a fresh id and only the answer bearing that id is taken |
//! | it exited while idle | noticed before the next request is written; that call starts a fresh one |
//! | it dies with a diagram in flight | that diagram's verdict is an `error` saying so; a fresh one is started in the background |
//! | a diagram outlives [`LIMIT`] (5 s) | that diagram's verdict is an `error` saying so; the program is killed and a fresh one started in the background |
//! | a piece is missing (Node, jsdom, the bundle, the program) | the call answers [`Unavailable`], naming the piece and the command that provides it; nothing is kept, so the next call looks again — installing the piece needs no restart |
//! | the caller goes away mid-call | nothing: the request runs to its end in its own task (MNG-20's rule), so the protocol is never left half-written; its answer is dropped |
//! | the service shuts down | [`Driver::shutdown`] kills the program; dropping the last [`Driver`] kills it too (`kill_on_drop`); and if the service dies outright, the program sees its stdin close and exits |
//!
//! The program's stderr is drained continuously into the log (a full pipe
//! would stall it) and never mixed with its stdout; the last few lines are
//! kept to say why it died, when it does.

use crate::types::Verdict;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

/// How long one diagram may take, from the moment it is written to the
/// program to its answer (the ticket's limit). mermaid parses a diagram in
/// single-digit milliseconds; a diagram that takes this long has hung it.
pub const LIMIT: Duration = Duration::from_secs(5);

/// How long the program may take to say `ready` — loading jsdom and the
/// 3.5 MB bundle, ~0.5 s warm, longer on a cold disk.
pub const START_LIMIT: Duration = Duration::from_secs(30);

/// The program, inside the node directory.
pub const PROGRAM: &str = "mermaid-parse.js";

/// The bundle the app serves (`frontend/src/mermaid.rs`'s `ASSET_URL`;
/// a test holds the two to one name). Upgrading mermaid upgrades both.
pub const BUNDLE: &str = "mermaid-11.17.2.min.js";

/// Where the pieces are.
#[derive(Debug, Clone)]
pub struct Config {
    /// The Node binary: a bare name is looked up on `PATH`.
    pub node: PathBuf,
    /// The program Node runs. [`Config::new`] puts it in the node
    /// directory, beside the `node_modules` holding jsdom.
    pub program: PathBuf,
    /// The mermaid bundle the program loads.
    pub bundle: PathBuf,
    /// The per-diagram limit ([`LIMIT`]).
    pub limit: Duration,
    /// The start limit ([`START_LIMIT`]).
    pub start_limit: Duration,
}

impl Config {
    pub fn new(
        node: impl Into<PathBuf>,
        node_dir: impl AsRef<Path>,
        bundle: impl Into<PathBuf>,
    ) -> Self {
        Self {
            node: node.into(),
            program: node_dir.as_ref().join(PROGRAM),
            bundle: bundle.into(),
            limit: LIMIT,
            start_limit: START_LIMIT,
        }
    }

    /// `diagrams/node` in this checkout — the program and its lockfile.
    pub fn default_node_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("node")
    }

    /// The vendored bundle in this checkout — the very file the app serves.
    pub fn default_bundle() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("diagrams/ is inside menger's checkout")
            .join("frontend")
            .join("vendor")
            .join("mermaid")
            .join(BUNDLE)
    }
}

impl Default for Config {
    /// `node` from `PATH`, this checkout's program and bundle, the limits.
    fn default() -> Self {
        Self::new("node", Self::default_node_dir(), Self::default_bundle())
    }
}

/// The service cannot ask mermaid anything: a piece is missing (or the
/// program could not start). The message names what and how to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable(pub String);

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The handle the activation holds: clones share one program.
#[derive(Clone)]
pub struct Driver {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    /// The running program, if any. The lock is the queue: one diagram in
    /// flight at a time, in arrival order (tokio's mutex is FIFO).
    program: Mutex<Option<Program>>,
    /// Request ids, never reused, so a stray answer can never be taken for
    /// another request's.
    next_id: AtomicU64,
    /// The running program's pid (0: none) — readable without the lock.
    pid: AtomicU32,
    /// Set by [`Driver::shutdown`]: nothing starts after it.
    closed: AtomicBool,
}

impl Driver {
    /// A driver for these pieces. Nothing starts until the first call.
    pub fn new(config: Config) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                program: Mutex::new(None),
                next_id: AtomicU64::new(1),
                pid: AtomicU32::new(0),
                closed: AtomicBool::new(false),
            }),
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// mermaid's verdict on one diagram — or [`Unavailable`] when no
    /// program can be had to ask.
    ///
    /// The request runs in a task of its own: a caller that goes away
    /// (a client unsubscribing mid-call) cannot cut a request off half
    /// written, which would leave the next caller reading a stream out of
    /// step. The answer is simply dropped.
    pub async fn parse(&self, source: String) -> Result<Verdict, Unavailable> {
        let inner = Arc::clone(&self.inner);
        match tokio::spawn(async move { inner.ask(source).await }).await {
            Ok(answer) => answer,
            Err(e) => Ok(Verdict::Error {
                message: format!("the diagrams service failed while parsing this diagram: {e}"),
            }),
        }
    }

    /// The running program's process id, if one is running — for the log,
    /// and for the tests that kill it.
    pub fn pid(&self) -> Option<u32> {
        match self.inner.pid.load(Ordering::SeqCst) {
            0 => None,
            pid => Some(pid),
        }
    }

    /// Kills the program (after the diagram in flight, if any) and starts
    /// no other. Every later call is [`Unavailable`].
    pub async fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        let mut slot = self.inner.program.lock().await;
        if let Some(program) = slot.take() {
            tracing::info!("stopping the Node program (pid {})", program.pid);
            program.kill().await;
        }
        self.inner.pid.store(0, Ordering::SeqCst);
    }
}

impl Inner {
    async fn ask(self: &Arc<Self>, source: String) -> Result<Verdict, Unavailable> {
        let mut slot = self.program.lock().await;

        // Killed, crashed or gone since the last call: replace it before
        // anything is written to it, so this call is not the one that
        // finds out.
        if let Some(status) = slot.as_mut().and_then(Program::exited) {
            let program = slot.take().expect("checked");
            tracing::warn!(
                "the Node program (pid {}) exited while idle ({status}){}; starting a fresh one",
                program.pid,
                program.stderr.last_words()
            );
            self.pid.store(0, Ordering::SeqCst);
        }
        if slot.is_none() {
            let program = self.start().await?;
            self.pid.store(program.pid, Ordering::SeqCst);
            *slot = Some(program);
        }

        let program = slot.as_mut().expect("started");
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let limit = self.config.limit;
        match tokio::time::timeout(limit, program.ask(id, &source)).await {
            Ok(Ok(verdict)) => Ok(verdict),
            Ok(Err(Lost)) => {
                let program = slot.take().expect("asked");
                self.pid.store(0, Ordering::SeqCst);
                let pid = program.pid;
                let (how, words) = program.reap().await;
                tracing::warn!("the Node program (pid {pid}) {how} while parsing a diagram{words}");
                self.restart_soon();
                Ok(Verdict::Error {
                    message: format!(
                        "the Node program {how} while parsing this diagram; the next call \
                         starts a fresh one{words}"
                    ),
                })
            }
            Err(_elapsed) => {
                let program = slot.take().expect("asked");
                self.pid.store(0, Ordering::SeqCst);
                tracing::warn!(
                    "a diagram took longer than {} to parse; killing the Node program (pid {})",
                    seconds(limit),
                    program.pid
                );
                program.kill().await;
                self.restart_soon();
                Ok(Verdict::Error {
                    message: format!(
                        "mermaid took longer than {} to parse this diagram (the limit); the \
                         Node program was stopped and is being restarted",
                        seconds(limit)
                    ),
                })
            }
        }
    }

    /// After a death or a timeout: start the next program now, behind the
    /// lock, so the next call finds it warm. A failure here is only
    /// logged — the next call tries again and reports it.
    fn restart_soon(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        tokio::spawn(async move {
            let mut slot = inner.program.lock().await;
            if slot.is_some() || inner.closed.load(Ordering::SeqCst) {
                return;
            }
            match inner.start().await {
                Ok(program) => {
                    inner.pid.store(program.pid, Ordering::SeqCst);
                    *slot = Some(program);
                }
                Err(Unavailable(why)) => {
                    tracing::warn!("could not restart the Node program: {why}")
                }
            }
        });
    }

    /// Starts the program and waits for its `ready`. Every way this fails
    /// is a missing piece, named.
    async fn start(&self) -> Result<Program, Unavailable> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Unavailable("the diagrams service is shutting down".into()));
        }
        let config = &self.config;
        // The files first: cheaper than starting Node to find out, and the
        // message can say exactly which flag moves them.
        if !config.program.is_file() {
            return Err(Unavailable(format!(
                "the diagrams service's Node program is not at {} — pass --node-dir \
                 <menger checkout>/diagrams/node (MENGER_DIAGRAMS_NODE_DIR)",
                config.program.display()
            )));
        }
        if !config.bundle.is_file() {
            return Err(Unavailable(format!(
                "the mermaid bundle is not at {} — the service loads the file the app serves, \
                 vendored in menger's checkout at frontend/vendor/mermaid/{BUNDLE}: pass --bundle \
                 <path> (MENGER_DIAGRAMS_BUNDLE), or restore it with `git checkout -- \
                 frontend/vendor/mermaid`",
                config.bundle.display()
            )));
        }

        let began = Instant::now();
        let mut child = Command::new(&config.node)
            .arg(&config.program)
            .arg("--bundle")
            .arg(&config.bundle)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| self.node_missing(e))?;
        let pid = child.id().unwrap_or(0);
        let stdin = child.stdin.take().expect("piped");
        let mut stdout = BufReader::new(child.stdout.take().expect("piped")).lines();
        let stderr = Tail::follow(child.stderr.take().expect("piped"), pid);

        // The first protocol line is `ready` or `fatal`. Anything else
        // before it is not the protocol, and is skipped.
        let hello = tokio::time::timeout(config.start_limit, async {
            loop {
                match stdout.next_line().await {
                    Ok(Some(line)) => match serde_json::from_str::<Hello>(&line) {
                        Ok(hello) => return Some(hello),
                        Err(_) => tracing::warn!(
                            pid,
                            "the Node program said, before `ready`: {line:.200}"
                        ),
                    },
                    Ok(None) | Err(_) => return None,
                }
            }
        })
        .await;

        let program = Program {
            child,
            stdin,
            stdout,
            stderr,
            pid,
        };
        match hello {
            Ok(Some(Hello::Ready {
                node,
                jsdom,
                load_ms,
            })) => {
                tracing::info!(
                    "the Node program is ready (pid {pid}; node {node}, jsdom {jsdom}): mermaid \
                     loaded in {load_ms} ms, {:.0} ms from spawn",
                    began.elapsed().as_secs_f64() * 1000.0
                );
                Ok(program)
            }
            Ok(Some(Hello::Fatal { missing, message })) => {
                tracing::warn!("the Node program cannot serve ({missing} missing): {message}");
                program.kill().await;
                Err(Unavailable(message))
            }
            Ok(None) => {
                let (how, words) = program.reap().await;
                Err(Unavailable(format!(
                    "the Node program ({} {}) {how} before it was ready{words}",
                    config.node.display(),
                    config.program.display()
                )))
            }
            Err(_elapsed) => {
                let words = program.stderr.last_words();
                program.kill().await;
                Err(Unavailable(format!(
                    "the Node program ({} {}) did not say it was ready within {}{words}",
                    config.node.display(),
                    config.program.display(),
                    seconds(config.start_limit)
                )))
            }
        }
    }

    /// Why Node could not be started, in words that say what to install.
    fn node_missing(&self, e: std::io::Error) -> Unavailable {
        let node = self.config.node.display();
        let install = "the diagrams service runs mermaid under Node.js (^22.22.2, ^24.15.0 or \
                       >=26): install it from https://nodejs.org or your package manager \
                       (`nodejs`)";
        Unavailable(match e.kind() {
            std::io::ErrorKind::NotFound if self.config.node.components().count() == 1 => format!(
                "Node.js is not installed: `{node}` is not on PATH — {install}, or pass --node \
                 <path> (MENGER_DIAGRAMS_NODE)"
            ),
            std::io::ErrorKind::NotFound => format!(
                "Node.js is not at {node} (--node / MENGER_DIAGRAMS_NODE) — {install}, or point \
                 --node at it"
            ),
            _ => format!("Node.js (`{node}`) could not be started: {e} — {install}"),
        })
    }
}

/// A whole number of seconds where it is one ("5 s"), milliseconds
/// otherwise (the tests' short limits).
fn seconds(d: Duration) -> String {
    if d.subsec_millis() == 0 {
        format!("{} s", d.as_secs())
    } else {
        format!("{} ms", d.as_millis())
    }
}

// -- the protocol --------------------------------------------------------------

/// The program's first line.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Hello {
    Ready {
        node: String,
        jsdom: String,
        load_ms: u64,
    },
    Fatal {
        missing: String,
        message: String,
    },
}

#[derive(Serialize)]
struct Request<'a> {
    id: u64,
    source: &'a str,
}

/// An answer: the request's id beside a verdict in the wire's own shape.
#[derive(Deserialize)]
struct Answer {
    id: Option<u64>,
    #[serde(flatten)]
    verdict: Verdict,
}

/// The program went away mid-request: its stdin or stdout closed.
struct Lost;

/// One running program and its pipes.
struct Program {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    stderr: Tail,
    pid: u32,
}

impl Program {
    /// Writes one request and reads to its answer. Only the line bearing
    /// this request's id is taken; any other line on stdout — not JSON, or
    /// another id — is logged and skipped.
    async fn ask(&mut self, id: u64, source: &str) -> Result<Verdict, Lost> {
        let mut line =
            serde_json::to_string(&Request { id, source }).expect("a request serializes");
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|_| Lost)?;
        self.stdin.flush().await.map_err(|_| Lost)?;
        loop {
            let Some(line) = self.stdout.next_line().await.map_err(|_| Lost)? else {
                return Err(Lost);
            };
            match serde_json::from_str::<Answer>(&line) {
                Ok(answer) if answer.id == Some(id) => return Ok(answer.verdict),
                Ok(answer) => tracing::warn!(
                    pid = self.pid,
                    "skipping an answer that is not this request's (id {:?}, waiting for {id})",
                    answer.id
                ),
                Err(_) => tracing::warn!(
                    pid = self.pid,
                    "skipping a line that is not the protocol: {line:.200}"
                ),
            }
        }
    }

    /// The exit status, if it has exited.
    fn exited(&mut self) -> Option<ExitStatus> {
        match self.child.try_wait() {
            Ok(status) => status,
            Err(e) => {
                tracing::warn!(
                    "could not ask after the Node program (pid {}): {e}",
                    self.pid
                );
                None
            }
        }
    }

    /// Kills it and waits for it to be gone.
    async fn kill(mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }

    /// After a lost request: how it ended ("exited (exit status: 3)",
    /// "exited (signal: 9 (SIGKILL))"), and its last words on stderr. A
    /// program that closed its pipes but lives on is killed.
    async fn reap(mut self) -> (String, String) {
        let status = match tokio::time::timeout(Duration::from_secs(1), self.child.wait()).await {
            Ok(Ok(status)) => format!("exited ({status})"),
            Ok(Err(e)) => format!("was lost ({e})"),
            Err(_) => {
                let _ = self.child.start_kill();
                let _ = self.child.wait().await;
                "stopped answering and was killed".to_string()
            }
        };
        // Its stderr closes with it; give the follower a moment to read
        // the last of it.
        tokio::time::sleep(Duration::from_millis(20)).await;
        (status, self.stderr.last_words())
    }
}

/// The program's stderr: drained into the log as it comes, the last few
/// lines kept for the message when it dies.
#[derive(Clone, Default)]
struct Tail(Arc<StdMutex<VecDeque<String>>>);

impl Tail {
    const LINES: usize = 6;
    const WIDTH: usize = 300;

    fn follow(stderr: ChildStderr, pid: u32) -> Self {
        let tail = Tail::default();
        let kept = tail.clone();
        tokio::spawn(async move {
            // Bytes, not `lines()`: a line that is not UTF-8 must not stop
            // the draining, or the pipe fills and the program stalls.
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf);
                        let line = line.trim_end();
                        tracing::debug!(target: "menger_diagrams::node", pid, "{line}");
                        kept.push(line);
                    }
                }
            }
        });
        tail
    }

    fn push(&self, line: &str) {
        if line.is_empty() {
            return;
        }
        let mut lines = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if lines.len() == Self::LINES {
            lines.pop_front();
        }
        lines.push_back(line.chars().take(Self::WIDTH).collect());
    }

    /// " — its stderr ended: …", or nothing when it said nothing.
    fn last_words(&self) -> String {
        let lines = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if lines.is_empty() {
            String::new()
        } else {
            format!(
                " — its stderr ended: {}",
                lines.iter().cloned().collect::<Vec<_>>().join(" | ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page's loader (`frontend/src/mermaid.rs`), as text: where the
    /// app fetches the bundle from.
    const APP_MERMAID_RS: &str = include_str!("../../frontend/src/mermaid.rs");

    /// The ticket's "Version drift" risk: the service loads the file the
    /// app serves. When the vendored bundle is upgraded, the app's
    /// `ASSET_URL` moves (its own test holds it to the file) and this fails
    /// until [`BUNDLE`] moves with it.
    #[test]
    fn the_default_bundle_is_the_one_the_app_serves() {
        let at = APP_MERMAID_RS
            .find("pub const ASSET_URL: &str = \"/app/vendor/")
            .expect("the app names its bundle");
        let rest = &APP_MERMAID_RS[at + "pub const ASSET_URL: &str = \"/app/vendor/".len()..];
        let served = &rest[..rest.find('"').unwrap()];
        assert_eq!(BUNDLE, served);
        let bundle = Config::default_bundle();
        assert!(bundle.is_file(), "{}", bundle.display());
        assert_eq!(bundle.file_name().unwrap(), served);
        assert!(bundle.ends_with("frontend/vendor/mermaid/mermaid-11.17.2.min.js"));
    }

    #[test]
    fn the_default_program_is_this_checkouts() {
        let config = Config::default();
        assert!(config.program.is_file(), "{}", config.program.display());
        assert!(config.program.ends_with("diagrams/node/mermaid-parse.js"));
        assert_eq!(config.node, PathBuf::from("node"));
        assert_eq!((config.limit, config.start_limit), (LIMIT, START_LIMIT));
        assert_eq!(LIMIT, Duration::from_secs(5), "the ticket's limit");
    }

    #[test]
    fn limits_read_as_seconds_or_milliseconds() {
        assert_eq!(seconds(Duration::from_secs(5)), "5 s");
        assert_eq!(seconds(Duration::from_millis(400)), "400 ms");
    }
}
