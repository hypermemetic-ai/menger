//! PERF-24: **the scale harness** — menger v2 at 10³ … 10⁹ tickets.
//!
//! Measurement only (`--features scale`); nothing that serves runs it.
//!
//! ```sh
//! cargo build --release --features scale --bin menger-scale
//! menger-scale gen   --db D/tickets.db --to 1000000 --templates T.json   # extend D's file to 10⁶
//! menger-scale bench --db D/tickets.db --bin perf/menger --out D/bench.json
//! menger-scale sizes --db D/tickets.db                                  # bytes by table and index
//! ```
//!
//! `scripts/scale/run.sh` runs the decades: each one a reflink copy of the
//! one before it, extended, then measured.
//!
//! **gen** writes tickets straight into a store file through the store's own
//! insert and search index (`menger_tickets_api::store::scale::Bulk`), then
//! opens it once as the server would ([`Store::open`]), which builds the
//! derived state from the tickets. The tickets are drawn from `--templates`,
//! a JSON array of real tickets (test's copy of prod's: kind, status,
//! severity, labels, blockers, commits and body together, so a body's size
//! goes with its kind), each made new: its own id, uid, parent in its
//! shortcode, blockers among the tickets before it, and a line of its own in
//! the body so the search index holds words no other ticket does. Shortcodes
//! grow as √N (132 at the templates' size), each with a scope root.
//!
//! **bench** starts the real server on the file (`--auth-provider key`, this
//! harness's key as `--admin`, its own ports), times the start to the first
//! answer, samples its RSS, and measures each hot call over the WebSocket —
//! p50, p99 and max — then a `watch` fan-out: `--watchers` sockets each
//! holding `tickets.watch`, a write, and the time until the last of them has
//! heard it.

use ed25519_dalek::SigningKey;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use menger_tickets_api::store::scale::{self, Bulk, Sampled};
use menger_tickets_api::store::Store;
use menger_tickets_api::types::Ticket;
use menger_tickets_api::Uid;
use plexus_auth_core::adapters::{key_principal, sign_key_proof};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The harness's key: the server's `--admin`, every socket's proof.
const ADMIN: [u8; 32] = [0x24; 32];

/// Tickets per transaction — and at most this many bytes of body in one.
const CHUNK_TICKETS: usize = 5_000;
const CHUNK_BYTES: usize = 256 << 20;

/// The templates' own size: test held 1976 tickets in 132 shortcodes.
const TEMPLATE_TICKETS: f64 = 1976.0;
const TEMPLATE_SHORTCODES: f64 = 132.0;

fn main() -> anyhow::Result<()> {
    // The store's own lines (the derived state's build, the migrations) on stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let need = |name: &str| -> anyhow::Result<String> { flag(name).ok_or_else(|| anyhow::anyhow!("{name} is required")) };
    match args.get(1).map(String::as_str) {
        Some("gen") => {
            let db = PathBuf::from(need("--db")?);
            let to: u64 = need("--to")?.parse()?;
            let templates = PathBuf::from(need("--templates")?);
            let seed: u64 = flag("--seed").map(|s| s.parse()).transpose()?.unwrap_or(24);
            gen(&db, to, &templates, seed)
        }
        Some("bench") => {
            let opts = BenchOpts {
                db: PathBuf::from(need("--db")?),
                bin: PathBuf::from(need("--bin")?),
                out: PathBuf::from(need("--out")?),
                port: flag("--port").map(|s| s.parse()).transpose()?.unwrap_or(4740),
                samples: flag("--samples").map(|s| s.parse()).transpose()?.unwrap_or(200),
                watchers: flag("--watchers").map(|s| s.parse()).transpose()?.unwrap_or(100),
            };
            tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build()?.block_on(bench(opts))
        }
        Some("sizes") => {
            let db = PathBuf::from(need("--db")?);
            let started = Instant::now();
            let sizes = scale::sizes(&db)?;
            let count = scale_count(&db)?;
            println!("{}", serde_json::to_string_pretty(&split_sizes(&sizes, count, started.elapsed()))?);
            Ok(())
        }
        _ => {
            eprintln!("usage: menger-scale gen|bench|sizes … (see the module doc)");
            std::process::exit(2)
        }
    }
}

fn scale_count(db: &Path) -> anyhow::Result<u64> {
    let b = Bulk::open_read(db)?;
    b.count()
}

/// The file's bytes split into the tickets table, its indexes, the search
/// index (FTS5's tables and `search_rows`) and everything else.
fn split_sizes(sizes: &[(String, u64)], count: u64, took: Duration) -> Value {
    let (mut table, mut indexes, mut fts, mut derived, mut other) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for (name, bytes) in sizes {
        let n = name.as_str();
        if n == "tickets" {
            table += bytes;
        } else if n.starts_with("sqlite_autoindex_tickets") || n.starts_with("tickets_") {
            indexes += bytes;
        } else if n.starts_with("search") || n.starts_with("sqlite_autoindex_search") {
            fts += bytes;
        } else if ["shortcode_runs", "forward_refs", "rollup_groups", "rollup_cross", "rollup_marks", "ready_counts", "status_counts", "cells", "cell_bits", "cell_counts", "tree_tallies", "tree_holders"]
            .iter()
            .any(|d| n == *d || n.starts_with(&format!("sqlite_autoindex_{d}")))
        {
            derived += bytes;
        } else {
            other += bytes;
        }
    }
    let total = table + indexes + fts + derived + other;
    let per = |b: u64| if count == 0 { 0.0 } else { b as f64 / count as f64 };
    json!({
        "tickets": count,
        "bytes": { "total": total, "table": table, "indexes": indexes, "fts": fts, "derived": derived, "other": other },
        "per_ticket": { "total": per(total), "table": per(table), "indexes": per(indexes), "fts": per(fts), "derived": per(derived), "other": per(other) },
        "objects": sizes.iter().map(|(n, b)| json!([n, b])).collect::<Vec<_>>(),
        "dbstat_ms": took.as_millis() as u64,
    })
}

// ---------------------------------------------------------------- gen

/// A real ticket, as the templates hold it.
#[derive(Deserialize)]
struct Template {
    title: String,
    body: String,
    kind: String,
    status: String,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    commits: Vec<String>,
}

/// How many shortcodes a store of `n` tickets has: 132 at 1976, as √N.
fn shortcodes_for(n: u64) -> u64 {
    ((TEMPLATE_SHORTCODES * (n as f64 / TEMPLATE_TICKETS).sqrt()).round() as u64).max(1)
}

/// The `i`th shortcode's code: `S` and letters, never more than six.
fn code(i: u64) -> String {
    let mut s = Vec::new();
    let mut x = i;
    loop {
        s.push(b'A' + (x % 26) as u8);
        x /= 26;
        if x == 0 {
            break;
        }
    }
    while s.len() < 3 {
        s.push(b'A');
    }
    s.reverse();
    format!("S{}", String::from_utf8(s).unwrap())
}

/// A shortcode's weight in [1, 8): sizes vary, as real ones do.
fn weight(i: u64) -> f64 {
    let h = (i.wrapping_add(1)).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    1.0 + 7.0 * ((h >> 11) as f64 / (1u64 << 53) as f64)
}

/// A bijection on 40 bits: the `n`th ticket's uid, no other's.
fn uid_of(n: u64) -> Uid {
    let mask = (1u64 << 40) - 1;
    let mut x = n & mask;
    x = (x.wrapping_mul(0x5_DEEC_E66D)) & mask; // odd: a bijection mod 2⁴⁰
    x ^= x >> 19;
    x = (x.wrapping_mul(0xB_F58_476D)) & mask; // odd
    x ^= x >> 17;
    Uid::from_bits(x)
}

fn gen(db: &Path, to: u64, templates: &Path, seed: u64) -> anyhow::Result<()> {
    let templates: Vec<Template> = serde_json::from_slice(&std::fs::read(templates)?)?;
    anyhow::ensure!(!templates.is_empty(), "no templates");
    let non_scope: Vec<usize> = (0..templates.len()).filter(|&i| templates[i].kind != "scope" || i % 3 == 0).collect();
    let started = Instant::now();
    let mut bulk = Bulk::open(db)?;
    let have = bulk.count()?;
    let mut last = bulk.last_seqs()?;
    let mut rng = StdRng::seed_from_u64(seed ^ to);
    eprintln!("gen: {} holds {have}; extending to {to}", db.display());

    // Every shortcode's target size at `to`, by weight.
    let shortcodes = shortcodes_for(to);
    let weights: Vec<f64> = (0..shortcodes).map(weight).collect();
    let total_w: f64 = weights.iter().sum();
    let mut want: Vec<(String, u64)> = Vec::with_capacity(shortcodes as usize);
    let mut planned = 0u64;
    for (i, w) in weights.iter().enumerate() {
        let c = code(i as u64);
        let target = ((to as f64) * w / total_w).floor() as u64;
        let held = *last.get(&c).unwrap_or(&0) as u64;
        let add = target.saturating_sub(held);
        planned += held + add;
        want.push((c, add));
    }
    // The remainder of the rounding, to the first shortcodes, one each.
    let mut short = to.saturating_sub(planned.max(have));
    for (_, add) in want.iter_mut() {
        if short == 0 {
            break;
        }
        *add += 1;
        short -= 1;
    }

    let base_unix: i64 = 1_760_000_000;
    let mut n = have; // tickets written, the uid counter
    let mut chunk: Vec<Ticket> = Vec::with_capacity(CHUNK_TICKETS);
    let mut chunk_bytes = 0usize;
    let (mut written_tickets, mut written_bytes) = (0u64, 0u64);
    let mut last_report = Instant::now();
    let hex = b"0123456789abcdef";
    for (c, add) in want {
        if add == 0 {
            continue;
        }
        let first = last.get(&c).copied().unwrap_or(0);
        // This shortcode's recent tickets in the chunk: (index in chunk, id, kind) — blockers and parents.
        let mut window: Vec<(usize, String, String)> = Vec::new();
        let root = format!("{c}-1");
        for k in 0..add as u32 {
            let seq = first + 1 + k;
            let id = format!("{c}-{seq}");
            let is_root = seq == 1;
            let t = if is_root {
                // A scope root: the template of a scope, as its shortcode's first.
                let scopes: Vec<usize> = (0..templates.len()).filter(|&i| templates[i].kind == "scope").collect();
                &templates[scopes[rng.gen_range(0..scopes.len().max(1))]]
            } else {
                &templates[non_scope[rng.gen_range(0..non_scope.len())]]
            };
            let (kind, status) = if is_root {
                ("scope".to_string(), "Epic".to_string())
            } else {
                (t.kind.clone(), t.status.clone())
            };
            let is_doc = kind == "doc";
            // Blockers: as many as the template had, among this shortcode's
            // tickets before it in this chunk, never a doc, never for a doc.
            let mut blocked_by: Vec<String> = Vec::new();
            if !is_doc && !t.blocked_by.is_empty() {
                let candidates: Vec<&(usize, String, String)> =
                    window.iter().rev().take(64).filter(|(_, _, k)| k != "doc").collect();
                for _ in 0..t.blocked_by.len().min(candidates.len()) {
                    let pick = candidates[rng.gen_range(0..candidates.len())];
                    if !blocked_by.contains(&pick.1) {
                        blocked_by.push(pick.1.clone());
                    }
                }
            }
            for b in &blocked_by {
                if let Some((at, _, _)) = window.iter().find(|(_, id2, _)| id2 == b) {
                    chunk[*at].unlocks.push(menger_tickets_api::TicketName::new(id.clone()));
                }
            }
            // Parent: none for the root; else the root, or a scope or build
            // before it in this chunk (nested, as real shortcodes are).
            let parent = if is_root {
                None
            } else {
                let hosts: Vec<&(usize, String, String)> =
                    window.iter().rev().take(200).filter(|(_, _, k)| k == "scope" || k == "build").collect();
                if hosts.is_empty() || rng.gen_bool(0.4) {
                    Some(root.clone())
                } else {
                    Some(hosts[rng.gen_range(0..hosts.len())].1.clone())
                }
            };
            let commits: Vec<String> = (0..t.commits.len().min(8))
                .map(|_| (0..7).map(|_| hex[rng.gen_range(0..16)] as char).collect())
                .collect();
            let created = base_unix + (n as i64);
            let words: Vec<String> = (0..3).map(|_| format!("w{:x}", rng.gen_range(0..(1u64 << 20)))).collect();
            let body = format!("{}\n\nscale {id} {}\n", t.body, words.join(" "));
            let ticket: Ticket = serde_json::from_value(json!({
                "uid": uid_of(n).as_str(),
                "id": id,
                "epic": c,
                "shortcode": c,
                "address": "",
                "seq": seq,
                "title": t.title,
                "status": status,
                "kind": kind,
                "severity": if is_doc { None } else { t.severity.clone() },
                "blocked_by": blocked_by,
                "unlocks": [],
                "parent": parent,
                "child_count": 0,
                "commits": commits,
                "labels": t.labels,
                "body": body,
                "created_unix": created,
                "updated_unix": created + rng.gen_range(0..86_400 * 30),
            }))?;
            chunk_bytes += ticket.body.len();
            window.push((chunk.len(), ticket.id.to_string(), kind));
            chunk.push(ticket);
            n += 1;
            if chunk.len() >= CHUNK_TICKETS || chunk_bytes >= CHUNK_BYTES {
                written_tickets += chunk.len() as u64;
                written_bytes += chunk_bytes as u64;
                bulk.append(&chunk)?;
                chunk.clear();
                chunk_bytes = 0;
                window.clear();
                if last_report.elapsed() > Duration::from_secs(30) {
                    let s = started.elapsed().as_secs_f64();
                    eprintln!(
                        "gen: {n} tickets ({written_tickets} new, {:.1} GB of body) in {s:.0} s — {:.0} tickets/s",
                        written_bytes as f64 / 1e9,
                        written_tickets as f64 / s
                    );
                    last_report = Instant::now();
                }
            }
        }
        last.insert(c, first + add as u32);
    }
    if !chunk.is_empty() {
        written_tickets += chunk.len() as u64;
        written_bytes += chunk_bytes as u64;
        bulk.append(&chunk)?;
    }
    let count = bulk.count()?;
    bulk.close()?;
    let insert = started.elapsed();
    eprintln!("gen: inserted {written_tickets} in {:.1} s; opening the store to build its derived state", insert.as_secs_f64());
    // The first open: migrations, and the derived state built from the tickets.
    let opened = Instant::now();
    let store = Store::open(db)?;
    drop(store);
    let derive = opened.elapsed();
    let report = json!({
        "tickets": count,
        "added": written_tickets,
        "body_bytes_added": written_bytes,
        "shortcodes": shortcodes,
        "insert_s": insert.as_secs_f64(),
        "derive_s": derive.as_secs_f64(),
        "total_s": started.elapsed().as_secs_f64(),
    });
    println!("{report}");
    Ok(())
}

// ---------------------------------------------------------------- bench

struct BenchOpts {
    db: PathBuf,
    bin: PathBuf,
    out: PathBuf,
    port: u16,
    samples: usize,
    watchers: usize,
}

fn admin_principal() -> String {
    key_principal(SigningKey::from_bytes(&ADMIN).verifying_key().as_bytes()).to_string()
}

fn proof(port: u16, agent: &str) -> String {
    let mut nonce = [0u8; 32];
    rand::thread_rng().fill(&mut nonce);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    sign_key_proof(&ADMIN, &format!("ws://127.0.0.1:{port}"), agent, &nonce, now)
}

async fn connect(port: u16, agent: &str) -> anyhow::Result<WsClient> {
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", proof(port, agent)))?);
    Ok(WsClientBuilder::default()
        .set_headers(h)
        .max_request_size(64 << 20)
        .max_response_size(512 << 20)
        .max_buffer_capacity_per_subscription(1 << 16)
        .request_timeout(Duration::from_secs(600))
        .build(format!("ws://127.0.0.1:{port}"))
        .await?)
}

/// One call, to its end: every item until the stream says it is done. The
/// error items, if any, come back too.
async fn call(c: &WsClient, method: &str, params: Value) -> anyhow::Result<(Vec<Value>, Option<Value>)> {
    let mut p = ObjectParams::new();
    p.insert("method", method)?;
    p.insert("params", params)?;
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await?;
    let mut out = Vec::new();
    let mut err = None;
    loop {
        match tokio::time::timeout(Duration::from_secs(600), s.next()).await {
            Ok(Some(Ok(item))) => match item["type"].as_str() {
                Some("data") => {
                    let content = &item["content"];
                    if content["type"] == "error" || content["type"] == "not_found" {
                        err.get_or_insert(content.clone());
                    }
                    out.push(content.clone());
                }
                Some("error") => {
                    err = Some(item.clone());
                    break;
                }
                _ => break,
            },
            Ok(Some(Err(e))) => anyhow::bail!("{method}: {e}"),
            Ok(None) => break,
            Err(_) => anyhow::bail!("{method}: timed out"),
        }
    }
    Ok((out, err))
}

struct Server {
    child: Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn rss_kb(pid: u32) -> (u64, u64) {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let field = |name: &str| -> u64 {
        s.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

#[derive(Default)]
struct Timings {
    ms: Vec<f64>,
    errors: usize,
    first_error: Option<String>,
    items: usize,
}

impl Timings {
    fn add(&mut self, t: Duration, r: &anyhow::Result<(Vec<Value>, Option<Value>)>) {
        self.ms.push(t.as_secs_f64() * 1000.0);
        match r {
            Ok((items, err)) => {
                self.items += items.len();
                if let Some(e) = err {
                    self.errors += 1;
                    self.first_error.get_or_insert_with(|| e.to_string().chars().take(300).collect());
                }
            }
            Err(e) => {
                self.errors += 1;
                self.first_error.get_or_insert_with(|| e.to_string());
            }
        }
    }

    fn summary(&self) -> Value {
        let mut v = self.ms.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |p: f64| -> f64 {
            if v.is_empty() {
                return 0.0;
            }
            let i = ((v.len() as f64 - 1.0) * p).round() as usize;
            v[i]
        };
        json!({
            "n": v.len(), "p50_ms": q(0.5), "p99_ms": q(0.99), "max_ms": v.last().copied().unwrap_or(0.0),
            "mean_ms": if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 },
            "errors": self.errors, "first_error": self.first_error, "items_per_call": if v.is_empty() { 0.0 } else { self.items as f64 / v.len() as f64 },
        })
    }
}

async fn bench(o: BenchOpts) -> anyhow::Result<()> {
    let count = scale_count(&o.db)?;
    let mut rng = StdRng::seed_from_u64(0x24);
    let sampled: Vec<Sampled> = scale::sample(&o.db, o.samples.max(50) * 2, |max| rng.gen_range(0..max))?;
    let work: Vec<&Sampled> = sampled.iter().filter(|s| s.kind != "doc" && s.kind != "scope").collect();
    let epics: Vec<String> = {
        let mut e: Vec<String> = sampled.iter().map(|s| s.epic.clone()).collect();
        e.sort();
        e.dedup();
        e
    };
    let dir = o.db.parent().unwrap().to_path_buf();
    // Two starts: the first a release's first on the file (COMPS-5's backfill
    // registers), the second what every later start costs — the one measured.
    let mut starts: Vec<Value> = Vec::new();
    let mut kept = None;
    for start in 0..2 {
        let log = dir.join(format!("server-{start}.log"));
        let mut cmd = Command::new(&o.bin);
        cmd.args(["--auth-provider", "key", "--admin", &admin_principal()])
            .args(["--no-shell", "--no-rest", "--no-mcp", "--port", &o.port.to_string()])
            .arg("--db-path")
            .arg(&o.db)
            .arg("--ui-db-path")
            .arg(dir.join("ui.db"))
            .arg("--media-db-path")
            .arg(dir.join("media.db"))
            .arg("--docs-db-path")
            .arg(dir.join("docs.db"))
            .arg("--quota-db-path")
            .arg(dir.join("quota.db"))
            .arg("--ui-author-key")
            .arg(dir.join("ui-author.key"))
            .arg("--data-dir")
            .arg(dir.join("data"))
            .args(["--max-writes-per-second", "100000", "--max-connections-per-principal", "100000", "--max-connections-per-agent", "100000"])
            .args(["--max-calls-per-connection", "100000", "--idle-timeout-secs", "0"])
            .env("PLEXUS_NO_REGISTRY", "1")
            .env("RUST_LOG", "warn,menger=info,menger_tickets_api=info")
        .env("NO_COLOR", "1")
            .env_remove("MENGER_DB_PATH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(&log)?));
        let spawned = Instant::now();
        let mut server = Server { child: cmd.spawn()? };
        let pid = server.child.id();
        let mut peak_during_start = 0u64;
        // The start: until a socket opens and `health` answers.
        let client = loop {
            if spawned.elapsed() > Duration::from_secs(4 * 3600) {
                anyhow::bail!("the server did not answer within 4 h");
            }
            let (_, hwm) = rss_kb(pid);
            peak_during_start = peak_during_start.max(hwm);
            if server.child.try_wait()?.is_some() {
                anyhow::bail!("the server exited during its start; see {}", log.display());
            }
            if let Ok(c) = connect(o.port, "scale").await {
                if let Ok((items, None)) = call(&c, "tickets.health", json!({})).await {
                    if !items.is_empty() {
                        break c;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let startup = spawned.elapsed();
        let (rss_start, hwm_start) = rss_kb(pid);
        eprintln!("bench: {count} tickets; start {start}: serving after {:.2} s, RSS {} MB", startup.as_secs_f64(), rss_start / 1024);
        starts.push(json!({ "startup_s": startup.as_secs_f64(), "rss_mb": rss_start as f64 / 1024.0, "peak_mb": hwm_start.max(peak_during_start) as f64 / 1024.0 }));
        if start == 0 {
            drop(client);
            drop(server);
            continue;
        }
        kept = Some((server, client, pid, startup, rss_start, hwm_start, peak_during_start, log));
    }
    let (server, client, pid, startup, rss_start, hwm_start, peak_during_start, log) = kept.expect("two starts");

    let n = o.samples;
    let mut results: Vec<(String, Value)> = Vec::new();
    macro_rules! measure {
        ($name:expr, $reps:expr, |$i:ident| $params:expr, $method:expr) => {{
            let mut t = Timings::default();
            // A warm-up call, not counted.
            {
                let $i = 0usize;
                let _ = call(&client, $method, $params).await;
            }
            for $i in 0..$reps {
                let at = Instant::now();
                let r = call(&client, $method, $params).await;
                t.add(at.elapsed(), &r);
            }
            let s = t.summary();
            eprintln!("bench: {:<22} {}", $name, s);
            results.push(($name.to_string(), s));
        }};
    }

    measure!("get_by_id", n, |i| json!({ "id": sampled[i % sampled.len()].id }), "tickets.get");
    measure!("get_by_uid", n, |i| json!({ "id": sampled[(i + 7) % sampled.len()].uid }), "tickets.get");
    measure!("list_first_page", n.min(100), |_i| json!({ "summary": true, "limit": 50 }), "tickets.list");
    // A deep page: after a cursor drawn at random — most of the store before it.
    measure!("list_deep_after", n.min(100), |i| json!({ "summary": true, "limit": 50, "after": sampled[i % sampled.len()].id }), "tickets.list");
    measure!("list_deep_at", n.min(50), |i| json!({ "summary": true, "limit": 50, "filter": "not archived", "at": (count / 2) + i as u64 }), "tickets.list");
    measure!("list_filtered", n.min(100), |_i| json!({ "summary": true, "limit": 50, "filter": "kind gap and severity High and not category done" }), "tickets.list");
    measure!("list_in_shortcode", n.min(100), |i| json!({ "summary": true, "limit": 50, "filter": format!("in {} and status Draft", epics[i % epics.len()]) }), "tickets.list");
    measure!("search_common", n.min(50), |_i| json!({ "query": "ticket", "limit": 20 }), "tickets.search");
    measure!("search_two_words", n.min(50), |_i| json!({ "query": "store cache", "limit": 20 }), "tickets.search");
    measure!("search_rare", n.min(50), |i| json!({ "query": sampled[i % sampled.len()].id.clone(), "limit": 20 }), "tickets.search");
    measure!("search_typo", n.min(50), |_i| json!({ "query": "plcement wave", "limit": 20 }), "tickets.search");
    measure!("search_prefix", n.min(50), |_i| json!({ "query": "plac", "limit": 20 }), "tickets.search");
    measure!("search_filtered", n.min(50), |_i| json!({ "query": "watch", "limit": 20, "filter": "kind gap" }), "tickets.search");
    measure!("nav_counts", n.min(100), |_i| json!({ "filter": "ready and kind build" }), "tickets.nav.counts");
    measure!("nav_counts_all", n.min(100), |_i| json!({}), "tickets.nav.counts");
    measure!("nav_facets", n.min(100), |_i| json!({ "filter": "kind build" }), "tickets.nav.facets");
    measure!("create", n, |i| json!({ "epic": epics[i % epics.len()], "title": format!("scale probe {i}"), "kind": "build", "body": "A probe of the scale harness.\n\n## Problem\n\nNone; it is measured." }), "tickets.create");
    measure!("update", n, |i| json!({ "id": work[i % work.len()].id, "title": format!("scale update {i}") }), "tickets.update");
    measure!("set_status", n, |i| json!({ "id": work[i % work.len()].id, "status": if i % 2 == 0 { "Coding" } else { "Ready" } }), "tickets.set_status");
    let (rss_reads, _) = rss_kb(pid);

    // The fan-out: `watchers` sockets each holding `tickets.watch`, then a
    // write, and when each of them heard it.
    let fanout = fan_out(&o, &client, &work).await?;
    eprintln!("bench: watch_fan_out {fanout}");
    let (rss_end, hwm_end) = rss_kb(pid);
    drop(client);
    drop(server);

    let log_text = format!("{}{}", std::fs::read_to_string(dir.join("server-0.log")).unwrap_or_default(), std::fs::read_to_string(&log).unwrap_or_default());
    let notable: Vec<&str> = log_text
        .lines()
        .filter(|l| !l.contains("backfill: registered") && (l.contains("SCALE-28") || l.contains("loaded") || l.contains("derived state") || l.contains("WebSocket:") || l.contains("COMPS-5") || l.contains("backfill") || l.contains("SEARCH-2") || l.contains("ORG-3") || l.contains("TICKETS-35") || l.contains("ERROR")))
        .take(60)
        .collect();
    let mut calls = serde_json::Map::new();
    for (k, v) in results {
        calls.insert(k, v);
    }
    calls.insert("watch_fan_out".into(), fanout);
    let report = json!({
        "tickets": count,
        "startup_s": startup.as_secs_f64(),
        "starts": starts,
        "rss_mb": { "after_start": rss_start as f64 / 1024.0, "peak_start": hwm_start.max(peak_during_start) as f64 / 1024.0, "after_reads_writes": rss_reads as f64 / 1024.0, "end": rss_end as f64 / 1024.0, "peak": hwm_end as f64 / 1024.0 },
        "calls": calls,
        "log": notable,
    });
    std::fs::write(&o.out, serde_json::to_string_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}

async fn fan_out(o: &BenchOpts, writer: &WsClient, work: &[&Sampled]) -> anyhow::Result<Value> {
    use tokio::sync::mpsc;
    let (tx, mut rx) = mpsc::unbounded_channel::<(usize, String, Instant)>();
    let mut held = Vec::new();
    for w in 0..o.watchers {
        let c = Arc::new(connect(o.port, &format!("watch{w}")).await?);
        let mut p = ObjectParams::new();
        p.insert("method", "tickets.watch")?;
        p.insert("params", json!({}))?;
        let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await?;
        let tx = tx.clone();
        held.push(c.clone());
        tokio::spawn(async move {
            while let Some(Ok(item)) = s.next().await {
                let content = &item["content"];
                if content["type"] == "status_changed" {
                    let id = content["ticket"]["id"].as_str().or(content["id"].as_str()).unwrap_or("").to_string();
                    let _ = tx.send((w, id, Instant::now()));
                }
            }
        });
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let rounds = 50usize;
    let mut last_ms = Timings::default();
    let mut each_ms = Timings::default();
    let mut missing = 0usize;
    for r in 0..rounds {
        while rx.try_recv().is_ok() {}
        let target = work[(r * 13 + 5) % work.len()];
        let sent = Instant::now();
        let res = call(writer, "tickets.set_status", json!({ "id": target.id, "status": if r % 2 == 0 { "Coding" } else { "In Review" } })).await;
        if let Err(e) = &res {
            eprintln!("fan-out write: {e}");
        }
        let mut heard = vec![false; o.watchers];
        let mut got = 0usize;
        let mut last = Duration::ZERO;
        let deadline = Instant::now() + Duration::from_secs(30);
        while got < o.watchers {
            match tokio::time::timeout_at(deadline.into(), rx.recv()).await {
                Ok(Some((w, _id, at))) => {
                    if !heard[w] {
                        heard[w] = true;
                        got += 1;
                        let d = at.duration_since(sent);
                        each_ms.ms.push(d.as_secs_f64() * 1000.0);
                        last = last.max(d);
                    }
                }
                _ => break,
            }
        }
        missing += o.watchers - got;
        last_ms.ms.push(last.as_secs_f64() * 1000.0);
    }
    drop(held);
    Ok(json!({ "watchers": o.watchers, "writes": rounds, "missed": missing, "last_watcher": last_ms.summary(), "each_delivery": each_ms.summary() }))
}
