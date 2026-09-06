#!/usr/bin/env node
// PERF-3: the magnitude ladder.
//
// For each magnitude 10^m this harness: creates a temp database on disk
// (never the user's), starts the release `menger` binary on ephemeral
// ports with `--seed-synthetic 10^m` (PERF-2), records the seed time,
// checkpoints the WAL the way a clean close would, restarts the server
// to record the startup load time and RSS, measures the REST paths
// (health, list + payload bytes, get, ready_set, blockers), drives the
// Leptos app in Playwright's chromium (load -> live, nav shortcodes,
// expanding the largest group, board render, ticket page, a curl-created
// ticket appearing live), measures create -> watch push latency over SSE,
// then stops the server and deletes the database. Every measurement has
// its own timeout and is recorded as a failure row when it exceeds it —
// the breaking point is the finding, never a hang.
//
// Raw JSON goes beside the report; the report is regenerated from every
// raw file present, so a single-magnitude re-run merges into the ladder.
//
//   node magnitude.mjs                      # the ladder 10^2..10^6
//   node magnitude.mjs --magnitudes 3       # one magnitude
//   node magnitude.mjs --smoke              # 10^2, one run, to perf/out/smoke
//
// Options: --magnitudes 2,3,4 · --runs N (override the per-magnitude
// schedule) · --date YYYY-MM-DD (report name) · --out DIR · --tmp DIR ·
// --timeout-scale X · --no-build · --keep-db · --headed ·
// --report-only (regenerate the report from the raw files + notes.md
// without measuring anything)

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

// ---------------------------------------------------------------------
// Configuration

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..");

function parseArgs(argv) {
  const o = {
    magnitudes: [2, 3, 4, 5, 6],
    runs: null,
    date: new Date().toISOString().slice(0, 10),
    out: null,
    tmp: path.join(REPO, "target", "perf-tmp"),
    timeoutScale: 1,
    build: true,
    keepDb: false,
    headed: false,
    smoke: false,
    seedValue: 1,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => argv[++i];
    if (a === "--magnitudes") o.magnitudes = next().split(",").map((s) => parseInt(s, 10));
    else if (a === "--runs") o.runs = parseInt(next(), 10);
    else if (a === "--date") o.date = next();
    else if (a === "--out") o.out = path.resolve(next());
    else if (a === "--tmp") o.tmp = path.resolve(next());
    else if (a === "--timeout-scale") o.timeoutScale = parseFloat(next());
    else if (a === "--seed-value") o.seedValue = parseInt(next(), 10);
    else if (a === "--no-build") o.build = false;
    else if (a === "--keep-db") o.keepDb = true;
    else if (a === "--headed") o.headed = true;
    else if (a === "--smoke") o.smoke = true;
    else if (a === "--report-only") o.reportOnly = true;
    else if (a === "-h" || a === "--help") {
      console.log(fs.readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 28).map((l) => l.replace(/^\/\/ ?/, "")).join("\n"));
      process.exit(0);
    } else throw new Error(`unknown option ${a}`);
  }
  if (o.smoke) {
    o.magnitudes = [2];
    o.runs = o.runs ?? 1;
    o.out = o.out ?? path.join(HERE, "out", "smoke");
  }
  o.out = o.out ?? path.join(REPO, "plans", "PERF", "artifacts", `magnitude-${o.date}`);
  return o;
}

/// Runs per measurement: >= 3 at the small magnitudes, fewer where a
/// single run is minutes.
function runsFor(m, override) {
  if (override) return override;
  return { 2: 5, 3: 5, 4: 3, 5: 2, 6: 1 }[m] ?? 1;
}

/// Per-measurement budget. Generous at the top so a slow-but-working
/// path is measured rather than cut off; a hang still ends.
function budgetMs(m, kind, scale) {
  const base = {
    seed: { 2: 60e3, 3: 60e3, 4: 120e3, 5: 600e3, 6: 1800e3 },
    load: { 2: 30e3, 3: 30e3, 4: 60e3, 5: 300e3, 6: 900e3 },
    rest: { 2: 30e3, 3: 30e3, 4: 60e3, 5: 180e3, 6: 600e3 },
    push: { 2: 30e3, 3: 30e3, 4: 30e3, 5: 60e3, 6: 120e3 },
    // A page that is not live after two or three minutes is not usable;
    // the budget exists to record that, not to wait for it.
    browser: { 2: 30e3, 3: 30e3, 4: 60e3, 5: 120e3, 6: 180e3 },
  }[kind];
  return Math.round((base[m] ?? base[6]) * scale);
}

// ---------------------------------------------------------------------
// Small utilities

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => performance.now();

class MeasureTimeout extends Error {
  constructor(ms) {
    super(`timed out after ${ms} ms`);
    this.name = "MeasureTimeout";
  }
}

/// Runs `fn` under a budget. Resolves to a result row: ok with `ms`, or
/// timeout/error with the reason. Never throws.
async function measure(name, ms, fn) {
  const t0 = now();
  let timer;
  const guard = new Promise((_, rej) => (timer = setTimeout(() => rej(new MeasureTimeout(ms)), ms)));
  try {
    const extra = await Promise.race([fn(), guard]);
    return { name, status: "ok", ms: +(now() - t0).toFixed(1), ...(extra ?? {}) };
  } catch (e) {
    const status = e instanceof MeasureTimeout ? "timeout" : "error";
    return { name, status, ms: +(now() - t0).toFixed(1), budget_ms: ms, reason: String(e && e.message ? e.message : e).split("\n")[0].slice(0, 300) };
  } finally {
    clearTimeout(timer);
  }
}

function median(xs) {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

function tcpUp(port) {
  return new Promise((resolve) => {
    const s = net.connect({ port, host: "127.0.0.1" });
    s.once("connect", () => {
      s.destroy();
      resolve(true);
    });
    s.once("error", () => resolve(false));
  });
}

function procStatus(pid) {
  try {
    const txt = fs.readFileSync(`/proc/${pid}/status`, "utf8");
    const grab = (k) => {
      const m = txt.match(new RegExp(`^${k}:\\s+(\\d+) kB`, "m"));
      return m ? parseInt(m[1], 10) * 1024 : null;
    };
    return { rss_bytes: grab("VmRSS"), peak_rss_bytes: grab("VmHWM") };
  } catch {
    return { rss_bytes: null, peak_rss_bytes: null };
  }
}

function fmtBytes(b) {
  if (b == null) return "—";
  if (b < 1024) return `${b} B`;
  if (b < 1024 ** 2) return `${(b / 1024).toFixed(1)} KiB`;
  if (b < 1024 ** 3) return `${(b / 1024 ** 2).toFixed(1)} MiB`;
  return `${(b / 1024 ** 3).toFixed(2)} GiB`;
}

function fmtMs(ms) {
  if (ms == null) return "—";
  if (ms < 10) return `${ms.toFixed(1)} ms`;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

function fmtInt(n) {
  return n == null ? "—" : n.toLocaleString("en-US");
}

// ---------------------------------------------------------------------
// The server under measurement

const BIN = path.join(REPO, "target", "release", "menger");

class Server {
  constructor(dbPath, extraArgs, envExtra = {}) {
    this.dbPath = dbPath;
    this.extraArgs = extraArgs;
    this.envExtra = envExtra;
    this.stderr = "";
    this.child = null;
    this.exited = null;
  }

  async start() {
    this.ws = await freePort();
    this.shell = await freePort();
    this.rest = await freePort();
    const env = { ...process.env, ...this.envExtra };
    delete env.MENGER_DB_PATH;
    delete env.MENGER_APP_DIST;
    delete env.RUST_LOG;
    delete env.MENGER_SEED_SYNTHETIC;
    delete env.MENGER_SEED_VALUE;
    delete env.MENGER_SEED_INTO_EXISTING;
    const args = [
      "--port", String(this.ws),
      "--shell-port", String(this.shell),
      "--rest-port", String(this.rest),
      "--db-path", this.dbPath,
      ...this.extraArgs,
    ];
    this.child = spawn(BIN, args, { env, stdio: ["ignore", "ignore", "pipe"] });
    this.child.stderr.on("data", (d) => (this.stderr += d.toString()));
    this.exited = new Promise((resolve) => this.child.on("exit", (code, sig) => resolve({ code, sig })));
    this.startedAt = now();
  }

  get pid() {
    return this.child?.pid;
  }

  alive() {
    return this.child && this.child.exitCode === null && this.child.signalCode === null;
  }

  restUrl(method) {
    return `http://127.0.0.1:${this.rest}/rest/tickets/${method}`;
  }

  appUrl(hash = "") {
    return `http://127.0.0.1:${this.shell}/app/${hash}`;
  }

  /// Waits until every listener answers and `health` reports the
  /// expected count (or the process dies). Returns ms since spawn.
  async waitLive(expectCount, budget) {
    const deadline = now() + budget;
    while (now() < deadline) {
      if (!this.alive()) throw new Error(`server exited at startup: ${this.stderr.trim().split("\n").pop()}`);
      if ((await tcpUp(this.rest)) && (await tcpUp(this.shell)) && (await tcpUp(this.ws))) {
        try {
          const h = await restJson(this.restUrl("health"), {}, 5000);
          const count = h?.data?.[0]?.ticket_count;
          if (expectCount == null || count === expectCount) return +(now() - this.startedAt).toFixed(1);
        } catch {
          /* not yet */
        }
      }
      await sleep(100);
    }
    throw new MeasureTimeout(budget);
  }

  async stop() {
    if (!this.child) return;
    if (this.alive()) this.child.kill("SIGKILL");
    await Promise.race([this.exited, sleep(10000)]);
  }
}

/// Spawns with a retry on a startup death (a free-port race).
async function startServer(dbPath, extraArgs, expectCount, budget, envExtra) {
  let last;
  for (let attempt = 0; attempt < 3; attempt++) {
    const s = new Server(dbPath, extraArgs, envExtra);
    await s.start();
    try {
      const wallMs = await s.waitLive(expectCount, budget);
      return { server: s, wallMs };
    } catch (e) {
      last = e;
      const refused = /refusing|Error:/.test(s.stderr);
      await s.stop();
      if (e instanceof MeasureTimeout || refused) throw e;
      console.warn(`  startup attempt ${attempt + 1} failed (${e.message}); retrying with fresh ports`);
    }
  }
  throw last;
}

// ---------------------------------------------------------------------
// REST clients

async function httpError(url, res) {
  const body = await res.text().catch(() => "");
  return new Error(`${path.basename(url)} -> HTTP ${res.status} ${body.slice(0, 160)}`);
}

async function restJson(url, params, timeoutMs) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params ?? {}),
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (!res.ok) throw await httpError(url, res);
  return res.json();
}

/// Counts regex matches in `carry + chunk` that END inside the new
/// chunk: a match wholly inside the carry was counted last time, and a
/// match straddling the boundary is seen exactly once. Exact as long as
/// the carry is longer than any match.
function* newMatches(re, carry, text) {
  for (const m of text.matchAll(re)) {
    if (m.index + m[0].length > carry.length) yield m;
  }
}

/// Streams a (possibly huge) REST response: counts bytes and events
/// without ever holding the whole body, and runs an optional scanner
/// over `(carry, text)` so a 700 MB `list` can still be sampled for
/// ids. Verifies the envelope's tail.
async function restStream(url, params, timeoutMs, scan) {
  const t0 = now();
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params ?? {}),
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (!res.ok) throw await httpError(url, res);
  const ttfb = +(now() - t0).toFixed(1);
  let bytes = 0;
  let events = 0;
  let carry = "";
  let tail = "";
  const dec = new TextDecoder();
  for await (const chunk of res.body) {
    bytes += chunk.length;
    const text = carry + dec.decode(chunk, { stream: true });
    // Count event objects by their type tag (every event carries one).
    for (const _ of newMatches(/"type":"/g, carry, text)) events++;
    if (scan) scan(carry, text);
    carry = text.slice(-8192);
    tail = text.slice(-16);
  }
  if (!/\]\s*\}\s*$/.test(tail)) throw new Error(`response did not end as a JSON envelope (tail ${JSON.stringify(tail)})`);
  return { bytes, events, ttfb_ms: ttfb };
}

/// The same logical call over the WebSocket transport (what
/// web/index.html speaks): one `menger.call` subscription, items
/// streamed one notification each until `done`. The control for the
/// REST gateway's buffering: the store's own cost of a full list, with
/// no envelope to collect. Counts items and bytes; runs `scan` per item.
function wsList(port, params, timeoutMs, scan) {
  return new Promise((resolve, reject) => {
    const t0 = now();
    const ws = new WebSocket(`ws://127.0.0.1:${port}`);
    let bytes = 0;
    let events = 0;
    let first = null;
    const fail = (e) => {
      try { ws.close(); } catch { /* ignore */ }
      reject(e instanceof Error ? e : new Error(String(e)));
    };
    const timer = setTimeout(() => fail(new MeasureTimeout(timeoutMs)), timeoutMs);
    ws.onerror = () => fail(new Error("websocket error"));
    ws.onopen = () => ws.send(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "menger.call", params: { method: "tickets.list", params: params ?? {} } }));
    ws.onmessage = (m) => {
      const text = typeof m.data === "string" ? m.data : String(m.data);
      bytes += text.length;
      let msg;
      try { msg = JSON.parse(text); } catch { return; }
      if (msg.error) return fail(new Error(`ws error: ${JSON.stringify(msg.error).slice(0, 160)}`));
      const item = msg.params?.result;
      if (!item) return;
      if (item.type === "data") {
        if (first == null) first = +(now() - t0).toFixed(1);
        events++;
        if (scan) scan("", text);
      } else if (item.type === "done") {
        clearTimeout(timer);
        ws.close();
        resolve({ bytes, events, ttfb_ms: first });
      } else if (item.type === "error") {
        fail(new Error(`stream error: ${JSON.stringify(item).slice(0, 160)}`));
      }
    };
  });
}

/// An open `tickets.watch` SSE stream (direct REST port, no proxy).
class Watch {
  constructor() {
    this.buf = "";
    this.waiters = [];
    this.events = [];
    this.closed = false;
  }
  static async open(url, params, timeoutMs) {
    const w = new Watch();
    w.ctl = new AbortController();
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(params ?? {}),
      signal: w.ctl.signal,
    });
    if (!res.ok) throw new Error(`watch -> HTTP ${res.status}`);
    if (!(res.headers.get("content-type") || "").startsWith("text/event-stream")) throw new Error("watch is not SSE");
    w.pump(res.body).catch(() => {});
    // Give the stream a beat to be fully established server-side.
    await sleep(Math.min(200, timeoutMs));
    return w;
  }
  async pump(body) {
    const dec = new TextDecoder();
    for await (const chunk of body) {
      this.buf += dec.decode(chunk, { stream: true });
      let i;
      while ((i = this.buf.indexOf("\n\n")) >= 0) {
        const frame = this.buf.slice(0, i);
        this.buf = this.buf.slice(i + 2);
        const line = frame.split("\n").find((l) => l.startsWith("data:"));
        if (!line) continue;
        try {
          const item = JSON.parse(line.slice(5).trim());
          const content = item.content ?? item;
          content._at = now();
          this.events.push(content);
          this.waiters.splice(0).forEach((w) => w());
        } catch {
          /* ignore */
        }
      }
    }
    this.closed = true;
  }
  /// Resolves with the first event matching `pred` (already received or
  /// arriving later).
  async next(pred, timeoutMs) {
    const deadline = now() + timeoutMs;
    for (;;) {
      const i = this.events.findIndex(pred);
      if (i >= 0) return this.events.splice(i, 1)[0];
      if (this.closed) throw new Error("watch stream closed");
      const left = deadline - now();
      if (left <= 0) throw new MeasureTimeout(timeoutMs);
      await Promise.race([new Promise((r) => this.waiters.push(r)), sleep(Math.min(left, 250))]);
    }
  }
  close() {
    this.ctl?.abort();
  }
}

/// The literal "curl-created ticket": curl when present, fetch otherwise.
function createViaCurl(server, params) {
  const url = server.restUrl("create");
  if (spawnSync("which", ["curl"]).status === 0) {
    const r = spawnSync("curl", ["-sS", "-X", "POST", "-H", "content-type: application/json", "-d", JSON.stringify(params), url], {
      encoding: "utf8",
      timeout: 30000,
    });
    if (r.status !== 0) throw new Error(`curl failed: ${r.stderr}`);
    return { tool: "curl", body: JSON.parse(r.stdout) };
  }
  return restJson(url, params, 30000).then((body) => ({ tool: "fetch", body }));
}

// ---------------------------------------------------------------------
// WAL checkpoint after the seed run (what a clean close does). The seed
// server is stopped with SIGKILL like every other server here, which
// leaves the whole seed in the WAL; without this, the reload would read
// through a WAL the size of the database and the first write would pay
// for checkpointing all of it — a harness artifact, not the app's cost.

async function checkpointWal(dbPath) {
  try {
    const { DatabaseSync } = await import("node:sqlite");
    const db = new DatabaseSync(dbPath);
    db.exec("PRAGMA wal_checkpoint(TRUNCATE);");
    db.close();
    return "node:sqlite";
  } catch (e) {
    const r = spawnSync("sqlite3", [dbPath, "PRAGMA wal_checkpoint(TRUNCATE);"], { encoding: "utf8" });
    if (r.status === 0) return "sqlite3";
    return `none (${String(e.message).split("\n")[0]})`;
  }
}

// ---------------------------------------------------------------------
// Machine facts for the report

function machineSpecs(browserVersion) {
  const first = (cmd, args) => {
    const r = spawnSync(cmd, args, { encoding: "utf8" });
    return r.status === 0 ? r.stdout.trim().split("\n")[0] : null;
  };
  const cpuinfo = fs.existsSync("/proc/cpuinfo") ? fs.readFileSync("/proc/cpuinfo", "utf8") : "";
  const meminfo = fs.existsSync("/proc/meminfo") ? fs.readFileSync("/proc/meminfo", "utf8") : "";
  const osr = fs.existsSync("/etc/os-release") ? fs.readFileSync("/etc/os-release", "utf8") : "";
  const kb = (k) => {
    const m = meminfo.match(new RegExp(`^${k}:\\s+(\\d+) kB`, "m"));
    return m ? parseInt(m[1], 10) * 1024 : null;
  };
  const disk = spawnSync("lsblk", ["-dno", "NAME,ROTA,SIZE,MODEL"], { encoding: "utf8" });
  const cargoToml = fs.readFileSync(path.join(REPO, "Cargo.toml"), "utf8");
  return {
    cpu: (cpuinfo.match(/^model name\s*:\s*(.+)$/m) || [])[1] ?? os.cpus()[0]?.model ?? "?",
    cores: os.cpus().length,
    mem_total_bytes: kb("MemTotal") ?? os.totalmem(),
    mem_available_bytes: kb("MemAvailable"),
    swap_total_bytes: kb("SwapTotal"),
    kernel: os.release(),
    os: (osr.match(/^PRETTY_NAME="?(.*?)"?$/m) || [])[1] ?? `${os.type()} ${os.release()}`,
    disk: disk.status === 0 ? disk.stdout.trim().split("\n").filter((l) => !/^zram/.test(l)).join("; ") : null,
    node: process.version,
    chromium: browserVersion,
    rustc: first("rustc", ["-V"]),
    menger: (cargoToml.match(/^version = "(.+?)"/m) || [])[1],
    commit: first("git", ["-C", REPO, "rev-parse", "--short", "HEAD"]),
    playwright: JSON.parse(fs.readFileSync(path.join(HERE, "node_modules", "playwright", "package.json"), "utf8")).version,
    server_profile: "release",
  };
}

// ---------------------------------------------------------------------
// One magnitude

async function runMagnitude(m, opts, browser) {
  const N = 10 ** m;
  const runs = runsFor(m, opts.runs);
  const B = (kind) => budgetMs(m, kind, opts.timeoutScale);
  const label = `1e${m}`;
  const tmpDir = fs.mkdtempSync(path.join(opts.tmp, `${label}-`));
  const dbPath = path.join(tmpDir, "tickets.db");
  const shots = path.join(opts.out, "shots");
  fs.mkdirSync(shots, { recursive: true });
  const R = { magnitude: m, n: N, runs, started_at: new Date().toISOString(), db_path: dbPath, measurements: {}, samples: {}, notes: [] };
  const rows = (name) => (R.measurements[name] ??= []);
  const log = (msg) => console.log(`[${label}] ${msg}`);
  let server = null;

  const summarize = (name, extraKeys = []) => {
    const rs = rows(name);
    const oks = rs.filter((r) => r.status === "ok");
    const s = { runs: rs.length, ok: oks.length, median_ms: median(oks.map((r) => r.ms)), min_ms: oks.length ? Math.min(...oks.map((r) => r.ms)) : null, max_ms: oks.length ? Math.max(...oks.map((r) => r.ms)) : null };
    for (const k of extraKeys) {
      const vals = oks.map((r) => r[k]).filter((v) => v != null);
      if (vals.length) s[k] = typeof vals[0] === "number" ? median(vals) : vals[0];
    }
    const fails = rs.filter((r) => r.status !== "ok");
    if (fails.length) s.failure = `${fails[0].status}: ${fails[0].reason}` + (fails.length > 1 ? ` (+${fails.length - 1} more)` : "");
    if (!rs.length) s.failure = "not run";
    return s;
  };

  try {
    // ---- 1. Seed -------------------------------------------------------
    log(`seeding ${fmtInt(N)} tickets (seed value ${opts.seedValue})`);
    {
      const r = await measure("seed", B("seed"), async () => {
        const { server: s, wallMs } = await startServer(dbPath, ["--seed-synthetic", String(N), "--seed-value", String(opts.seedValue)], N, B("seed"));
        const line = s.stderr.match(/seeded (\d+) synthetic tickets in ([\d.]+) ms \(seed value \d+, (\d+) epics, (\d+) blocked_by edges; generate ([\d.]+) ms, persist ([\d.]+) ms\)/);
        const rss = procStatus(s.pid);
        await s.stop();
        if (!line) throw new Error("seed log line not found in server stderr");
        return {
          ms: parseFloat(line[2]),
          seed_logged_ms: parseFloat(line[2]),
          epics: parseInt(line[3], 10),
          edges: parseInt(line[4], 10),
          generate_ms: parseFloat(line[5]),
          persist_ms: parseFloat(line[6]),
          wall_to_live_ms: wallMs,
          rss_after_seed_bytes: rss.rss_bytes,
          peak_rss_seed_bytes: rss.peak_rss_bytes,
        };
      });
      // The logged seed time is the measurement; the wall clock includes process start and probing.
      if (r.status === "ok") r.ms = r.seed_logged_ms;
      rows("seed").push(r);
      log(`seed: ${r.status} ${fmtMs(r.ms)}${r.status === "ok" ? ` (${r.epics} epics, ${fmtInt(r.edges)} edges; generate ${fmtMs(r.generate_ms)}, persist ${fmtMs(r.persist_ms)})` : ` — ${r.reason}`}`);
      if (r.status !== "ok") throw new Error(`seed failed: ${r.reason}`);
      R.samples.epics = r.epics;
      R.samples.edges = r.edges;
      R.db_bytes_after_seed = fs.existsSync(dbPath) ? fs.statSync(dbPath).size + (fs.existsSync(dbPath + "-wal") ? fs.statSync(dbPath + "-wal").size : 0) : null;
      R.wal_checkpoint = await checkpointWal(dbPath);
      R.db_bytes = fs.statSync(dbPath).size;
      log(`database ${fmtBytes(R.db_bytes)} (WAL checkpoint via ${R.wal_checkpoint})`);
    }

    // ---- 2. Startup load (restart on the seeded database) ---------------
    for (let i = 0; i < runs; i++) {
      if (server) {
        await server.stop();
        server = null;
      }
      const r = await measure("startup_load", B("load"), async () => {
        const { server: s, wallMs } = await startServer(dbPath, [], N, B("load"));
        server = s;
        const line = s.stderr.match(/loaded (\d+) tickets from the database in ([\d.]+) ms/);
        if (!line) throw new Error("load log line not found");
        await sleep(300); // let the allocator settle before reading RSS
        const rss = procStatus(s.pid);
        return { ms: parseFloat(line[2]), load_logged_ms: parseFloat(line[2]), wall_to_live_ms: wallMs, rss_after_load_bytes: rss.rss_bytes };
      });
      if (r.status === "ok") r.ms = r.load_logged_ms;
      rows("startup_load").push(r);
      log(`startup load ${i + 1}/${runs}: ${r.status} ${fmtMs(r.ms)} (wall ${fmtMs(r.wall_to_live_ms)}; RSS ${fmtBytes(r.rss_after_load_bytes)})`);
      if (r.status !== "ok") throw new Error(`startup load failed: ${r.reason}`);
    }
    R.rss_after_load_bytes = procStatus(server.pid).rss_bytes;

    // ---- 3. REST ---------------------------------------------------------
    for (let i = 0; i < runs; i++) {
      rows("rest_health").push(await measure("rest_health", B("rest"), async () => {
        const h = await restJson(server.restUrl("health"), {}, B("rest"));
        if (h?.data?.[0]?.ticket_count !== N) throw new Error(`health count ${h?.data?.[0]?.ticket_count} != ${N}`);
        return { bytes: JSON.stringify(h).length };
      }));
    }
    log(`health: ${fmtMs(summarize("rest_health").median_ms)}`);

    // The samples every later measurement needs — per-epic sizes (the
    // largest nav group) and a ticket with several blockers — come from
    // scanning a streamed list. The gateway serializes keys
    // alphabetically, so a ticket reads `"blocked_by":[…],"body":"…",
    // "created_unix":…,"epic":"X","id":"X-N",…`.
    const epicCounts = new Map();
    const multi = [];
    let scanned = false;
    const scanner = (carry, text) => {
      for (const mm of newMatches(/"epic":"([A-Z]+)"/g, carry, text)) epicCounts.set(mm[1], (epicCounts.get(mm[1]) || 0) + 1);
      if (multi.length < 20) {
        for (const mm of newMatches(/"blocked_by":\["([^\]]*)"\],"body":"(?:[^"\\]|\\.)*","created_unix":\d+,"epic":"[A-Z]+","id":"([A-Z]+-\d+)"/g, carry, text)) {
          const blockers = mm[1].split('","');
          if (blockers.length >= 2 && !multi.some((x) => x.id === mm[2])) multi.push({ id: mm[2], blockers });
        }
      }
    };
    const scanOnce = (fn) => (carry, text) => { if (!scanned) fn(carry, text); };

    // list over WebSocket first (streamed — the control, and the scan
    // source that works at every magnitude).
    for (let i = 0; i < runs; i++) {
      const r = await measure("ws_list", B("rest"), async () => {
        const out = await wsList(server.ws, {}, B("rest"), scanOnce(scanner));
        if (out.events !== N) throw new Error(`ws list streamed ${out.events} items, expected ${N}`);
        return out;
      });
      if (r.status === "ok") scanned = true;
      else { epicCounts.clear(); multi.length = 0; }
      rows("ws_list").push(r);
      log(`ws list ${i + 1}/${runs}: ${r.status} ${fmtMs(r.ms)}${r.bytes ? ` ${fmtBytes(r.bytes)}` : ""}${r.reason ? ` — ${r.reason}` : ""}`);
    }

    // list over REST: latency + payload bytes (streamed, counted).
    for (let i = 0; i < runs; i++) {
      const r = await measure("rest_list", B("rest"), async () => {
        const out = await restStream(server.restUrl("list"), {}, B("rest"), scanOnce(scanner));
        if (out.events !== N) throw new Error(`list returned ${out.events} events, expected ${N}`);
        return out;
      });
      if (r.status === "ok") scanned = true;
      else if (!scanned) { epicCounts.clear(); multi.length = 0; }
      rows("rest_list").push(r);
      log(`list ${i + 1}/${runs}: ${r.status} ${fmtMs(r.ms)}${r.bytes ? ` ${fmtBytes(r.bytes)}` : ""}${r.reason ? ` — ${r.reason}` : ""}`);
    }

    let largest = null;
    for (const [epic, size] of epicCounts) if (!largest || size > largest.size) largest = { epic, size };
    R.samples.largest_epic = largest;
    R.samples.epic_count_seen = epicCounts.size;
    R.samples.multi_blocker = multi[0] ?? null;
    if (!largest) R.notes.push("largest epic unknown (no list succeeded); nav expansion measured on the first group instead");
    log(`largest epic: ${largest ? `${largest.epic} (${fmtInt(largest.size)} tickets)` : "unknown"}; sample with blockers: ${multi[0] ? `${multi[0].id} (${multi[0].blockers.length})` : "none"}`);

    const sampleId = multi[0]?.id ?? (largest ? `${largest.epic}-1` : null);
    for (let i = 0; i < runs; i++) {
      rows("rest_get").push(await measure("rest_get", B("rest"), async () => {
        if (!sampleId) throw new Error("no sample id (list failed)");
        const g = await restJson(server.restUrl("get"), { id: sampleId }, B("rest"));
        if (g?.data?.[0]?.type !== "ticket") throw new Error(`get ${sampleId}: ${JSON.stringify(g).slice(0, 120)}`);
        return { id: sampleId, bytes: JSON.stringify(g).length };
      }));
      rows("rest_ready_set").push(await measure("rest_ready_set", B("rest"), async () => {
        const out = await restStream(server.restUrl("ready_set"), {}, B("rest"));
        if (out.events === 0) throw new Error("ready_set empty");
        return out;
      }));
      rows("rest_blockers").push(await measure("rest_blockers", B("rest"), async () => {
        if (!multi[0]) throw new Error("no multi-blocker sample (list failed)");
        const b = await restJson(server.restUrl("blockers"), { id: multi[0].id }, B("rest"));
        const got = (b?.data ?? []).filter((e) => e.type === "blocker").length;
        if (got !== multi[0].blockers.length) throw new Error(`blockers ${multi[0].id}: ${got} resolved, expected ${multi[0].blockers.length}`);
        return { id: multi[0].id, count: got, bytes: JSON.stringify(b).length };
      }));
    }
    log(`get ${fmtMs(summarize("rest_get").median_ms)} · ready_set ${fmtMs(summarize("rest_ready_set").median_ms)} (${fmtInt(summarize("rest_ready_set", ["events"]).events)} tickets) · blockers ${fmtMs(summarize("rest_blockers").median_ms)}`);
    R.rss_after_rest_bytes = procStatus(server.pid).rss_bytes;

    // ---- 4. Browser --------------------------------------------------------
    const expectGroups = R.samples.epics;
    let coldFailed = null;
    for (let i = 0; i < runs; i++) {
      const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
      const page = await ctx.newPage();
      const pageErrors = [];
      page.on("pageerror", (e) => pageErrors.push(String(e.message).slice(0, 200)));
      page.on("crash", () => pageErrors.push("renderer crashed"));
      page.on("console", (msg) => {
        const t = msg.text();
        if (/session ended|panicked|RuntimeError|out of memory/i.test(t) && pageErrors.length < 5) pageErrors.push(t.slice(0, 200));
      });
      /// Why the page is not live: the app's own banner or console line.
      const diagnose = async () => {
        const banner = await page.textContent("#error .msg", { timeout: 1000 }).catch(() => null);
        const conn = await page.textContent("#conn", { timeout: 1000 }).catch(() => null);
        return [conn && `conn="${conn}"`, banner && `banner="${banner.slice(0, 120)}"`, pageErrors[0] && `page: ${pageErrors[0]}`].filter(Boolean).join("; ");
      };
      let tGoto = null;
      const sinceGoto = () => +(now() - tGoto).toFixed(1);
      const rowsAtGoto = (name, r) => {
        // Browser measurements are reported as time since navigation began, not since the measurement started.
        if (r.status === "ok" && r.since_goto_ms != null) r.ms = r.since_goto_ms;
        rows(name).push(r);
        return r;
      };

      // load -> live
      let r = rowsAtGoto("app_load_live", await measure("app_load_live", B("browser"), async () => {
        tGoto = now();
        await page.goto(server.appUrl(), { waitUntil: "commit", timeout: B("browser") });
        await page.waitForSelector("#conn.live", { timeout: B("browser"), state: "attached" });
        const since = sinceGoto();
        const health = await page.textContent("#health").catch(() => null);
        return { since_goto_ms: since, health_text: health };
      }));
      if (r.status !== "ok") {
        const why = await diagnose();
        if (why) r.reason = `${r.reason} [${why}]`.slice(0, 400);
      }
      log(`browser ${i + 1}/${runs}: load->live ${r.status} ${fmtMs(r.ms)}${r.reason ? ` — ${r.reason}` : ""}`);
      if (r.status !== "ok") {
        const why = `prerequisite load->live ${r.status}: ${r.reason}`;
        for (const name of ["nav_groups_visible", "board_render", "nav_expand_largest", "ticket_page_from_board", "live_create_appears"]) {
          rows(name).push({ name, status: "failed", ms: null, reason: why });
        }
        if (i === 0) await page.screenshot({ path: path.join(shots, `${label}-board.png`), timeout: 30000 }).catch(() => {});
        await ctx.close().catch(() => {});
      } else {
        // nav shortcodes visible (every epic group row present)
        r = rowsAtGoto("nav_groups_visible", await measure("nav_groups_visible", B("browser"), async () => {
          await page.waitForFunction((k) => document.querySelectorAll("#nav .grouprow").length >= k, expectGroups, { timeout: B("browser"), polling: 50 });
          return { since_goto_ms: sinceGoto(), groups: await page.evaluate(() => document.querySelectorAll("#nav .grouprow").length) };
        }));
        log(`  nav groups: ${r.status} ${fmtMs(r.ms)} (${r.groups ?? "?"} groups)`);

        // board render (every row in the DOM)
        r = rowsAtGoto("board_render", await measure("board_render", B("browser"), async () => {
          await page.waitForFunction((n) => document.querySelectorAll("tbody#board tr").length >= n, N, { timeout: B("browser"), polling: 50 });
          const dom = await page.evaluate(() => document.getElementsByTagName("*").length);
          return { since_goto_ms: sinceGoto(), dom_nodes: dom, rows: N };
        }));
        log(`  board render: ${fmtMs(r.ms)} ${r.status}${r.dom_nodes ? ` (${fmtInt(r.dom_nodes)} DOM nodes)` : ""}${r.reason ? ` — ${r.reason}` : ""}`);
        const boardMem = await browserMemory(page).catch(() => null);
        if (boardMem) rows("board_render").at(-1).memory = boardMem;

        // expand the largest group
        const target = largest?.epic ?? (await page.evaluate(() => document.querySelector("#nav .group")?.getAttribute("data-group")));
        const targetSize = largest?.size ?? null;
        r = rows("nav_expand_largest").push(await measure("nav_expand_largest", B("browser"), async () => {
          const sel = `#nav .group[data-group="${target}"] .grouprow`;
          await page.waitForSelector(sel, { timeout: 5000, state: "attached" });
          const t0 = now();
          await page.click(sel, { timeout: 5000, force: true });
          await page.waitForFunction(({ g, size }) => {
            const grp = document.querySelector(`#nav .group[data-group="${g}"]`);
            if (!grp || grp.classList.contains("collapsed")) return false;
            const leaves = grp.querySelectorAll(".kids .leaf").length;
            const first = grp.querySelector(".kids .leaf");
            return leaves >= (size ?? 1) && first && first.getClientRects().length > 0;
          }, { g: target, size: targetSize }, { timeout: B("browser"), polling: 50 });
          const leaves = await page.evaluate((g) => document.querySelectorAll(`#nav .group[data-group="${g}"] .kids .leaf`).length, target);
          return { ms: +(now() - t0).toFixed(1), epic: target, leaves, dom_nodes: await page.evaluate(() => document.getElementsByTagName("*").length) };
        })) && rows("nav_expand_largest").at(-1);
        log(`  expand ${target}: ${r.status} ${fmtMs(r.ms)}${r.leaves ? ` (${fmtInt(r.leaves)} leaves)` : ""}${r.reason ? ` — ${r.reason}` : ""}`);
        if (i === 0) await page.screenshot({ path: path.join(shots, `${label}-board.png`), timeout: 60000 }).catch((e) => R.notes.push(`screenshot failed: ${e.message.split("\n")[0]}`));

        // ticket page from the loaded board (warm: rows already in the store)
        r = rows("ticket_page_from_board").push(await measure("ticket_page_from_board", B("browser"), async () => {
          if (!sampleId) throw new Error("no sample id");
          const t0 = now();
          await page.evaluate((id) => { location.hash = `#/ticket/${id}`; }, sampleId);
          await page.waitForFunction(({ id, nb }) => {
            const h = document.querySelector("#ticket-page article.ticket h2 .id");
            if (!h || h.textContent !== id) return false;
            const li = document.querySelectorAll("#ticket-page #blockers li a, #ticket-page #blockers li .dangling").length;
            return nb === 0 ? !!document.querySelector("#ticket-page section.dag") : li >= nb;
          }, { id: sampleId, nb: multi[0]?.blockers.length ?? 0 }, { timeout: B("browser"), polling: 50 });
          return { ms: +(now() - t0).toFixed(1), id: sampleId };
        })) && rows("ticket_page_from_board").at(-1);
        log(`  ticket page (from board): ${r.status} ${fmtMs(r.ms)}${r.reason ? ` — ${r.reason}` : ""}`);
        if (i === 0) await page.screenshot({ path: path.join(shots, `${label}-ticket.png`), timeout: 60000 }).catch(() => {});

        // back to the board, then a curl-created ticket appears live
        r = rows("live_create_appears").push(await measure("live_create_appears", B("browser"), async () => {
          await page.evaluate(() => { location.hash = "#/"; });
          await page.waitForSelector("tbody#board", { timeout: B("browser"), state: "attached" });
          const t0 = now();
          const { tool, body } = await createViaCurl(server, { epic: "PROBE", title: `live probe ${label} run ${i + 1}`, kind: i === 0 ? "scope" : "gap" });
          const id = body?.data?.[0]?.ticket?.id;
          if (!id) throw new Error(`create failed: ${JSON.stringify(body).slice(0, 160)}`);
          const tCreated = now();
          await page.waitForFunction((id) => !!document.querySelector(`tbody#board tr[data-ticket-id="${id}"]`) && !!document.querySelector(`#nav a[data-nav-id="${id}"]`), id, { timeout: B("browser"), polling: 25 });
          return { ms: +(now() - t0).toFixed(1), create_ms: +(tCreated - t0).toFixed(1), id, tool };
        })) && rows("live_create_appears").at(-1);
        log(`  live create appears: ${r.status} ${fmtMs(r.ms)}${r.id ? ` (${r.id} via ${r.tool}, create ${fmtMs(r.create_ms)})` : ""}${r.reason ? ` — ${r.reason}` : ""}`);
        if (pageErrors.length) R.notes.push(`page errors run ${i + 1}: ${pageErrors.slice(0, 3).join(" | ")}`);
        await ctx.close().catch(() => {});
      }

      // ticket page, cold deep link (its own page: includes the app's load)
      if (coldFailed) {
        rows("ticket_page_cold").push({ name: "ticket_page_cold", status: "failed", ms: null, reason: `not retried after run ${coldFailed.run}: ${coldFailed.reason}` });
      } else {
        const ctx2 = await browser.newContext({ viewport: { width: 1400, height: 900 } });
        const p2 = await ctx2.newPage();
        const r2 = await measure("ticket_page_cold", B("browser"), async () => {
          if (!sampleId) throw new Error("no sample id");
          const t0 = now();
          await p2.goto(server.appUrl(`#/ticket/${sampleId}`), { waitUntil: "commit", timeout: B("browser") });
          await p2.waitForFunction(({ id, nb }) => {
            const h = document.querySelector("#ticket-page article.ticket h2 .id");
            if (!h || h.textContent !== id) return false;
            const li = document.querySelectorAll("#ticket-page #blockers li a, #ticket-page #blockers li .dangling").length;
            return nb === 0 ? !!document.querySelector("#ticket-page section.dag") : li >= nb;
          }, { id: sampleId, nb: multi[0]?.blockers.length ?? 0 }, { timeout: B("browser"), polling: 50 });
          return { ms: +(now() - t0).toFixed(1), id: sampleId, live: !!(await p2.$("#conn.live")) };
        });
        if (r2.status !== "ok") {
          const conn = await p2.textContent("#conn", { timeout: 1000 }).catch(() => null);
          const banner = await p2.textContent("#error .msg", { timeout: 1000 }).catch(() => null);
          if (conn || banner) r2.reason = `${r2.reason} [conn="${conn}"${banner ? `; banner="${banner.slice(0, 120)}"` : ""}]`.slice(0, 400);
          coldFailed = { run: i + 1, reason: `${r2.status}: ${r2.reason}` };
        }
        rows("ticket_page_cold").push(r2);
        log(`  ticket page (cold deep link): ${r2.status} ${fmtMs(r2.ms)}${r2.reason ? ` — ${r2.reason}` : ""}`);
        if (i === 0) await p2.screenshot({ path: path.join(shots, `${label}-ticket-cold.png`), timeout: 60000 }).catch(() => {});
        await ctx2.close().catch(() => {});
      }
    }
    R.rss_after_browser_bytes = procStatus(server.pid).rss_bytes;

    // ---- 5. create -> watch push (SSE, direct REST) ------------------------
    {
      let watch = null;
      try {
        watch = await Watch.open(server.restUrl("watch"), {}, B("push"));
      } catch (e) {
        R.notes.push(`watch open failed: ${e.message}`);
      }
      for (let i = 0; i < runs; i++) {
        rows("create_watch_push").push(await measure("create_watch_push", B("push"), async () => {
          if (!watch) throw new Error("watch stream not open");
          const t0 = now();
          const created = await restJson(server.restUrl("create"), { epic: "PROBE", title: `push probe ${label} ${i + 1}`, kind: i === 0 ? "scope" : "build" }, B("push"));
          const id = created?.data?.[0]?.ticket?.id;
          if (!id) throw new Error(`create failed: ${JSON.stringify(created).slice(0, 160)}`);
          const tAck = now();
          const ev = await watch.next((e) => e.type === "created" && e.ticket?.id === id, B("push"));
          return { ms: +(ev._at - t0).toFixed(1), create_ack_ms: +(tAck - t0).toFixed(1), id };
        }));
      }
      watch?.close();
      log(`create->watch push: ${fmtMs(summarize("create_watch_push").median_ms)} (create ack ${fmtMs(summarize("create_watch_push", ["create_ack_ms"]).create_ack_ms)})`);
    }
    R.rss_end_bytes = procStatus(server.pid).rss_bytes;
    R.peak_rss_bytes = procStatus(server.pid).peak_rss_bytes;
  } catch (e) {
    R.aborted = String(e.message ?? e);
    console.error(`[${label}] aborted: ${R.aborted}`);
  } finally {
    if (server) await server.stop();
    if (!opts.keepDb) fs.rmSync(tmpDir, { recursive: true, force: true });
    else R.notes.push(`database kept at ${tmpDir}`);
  }

  // Fill every expected measurement so the report never has a blank cell.
  for (const name of MEASUREMENTS.map((x) => x.key)) {
    if (!rows(name).length) rows(name).push({ name, status: "failed", ms: null, reason: R.aborted ? `not reached: ${R.aborted}` : "not run" });
  }
  R.summary = Object.fromEntries(MEASUREMENTS.map((x) => [x.key, summarize(x.key, x.extra ?? [])]));
  R.finished_at = new Date().toISOString();
  return R;
}

async function browserMemory(page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Performance.enable");
  const { metrics } = await cdp.send("Performance.getMetrics");
  const get = (n) => metrics.find((x) => x.name === n)?.value ?? null;
  await cdp.detach().catch(() => {});
  return { js_heap_used_bytes: get("JSHeapUsedSize"), js_heap_total_bytes: get("JSHeapTotalSize"), dom_nodes: get("Nodes") };
}

// ---------------------------------------------------------------------
// Report

const MEASUREMENTS = [
  { key: "seed", label: "seed time (logged)", extra: ["epics", "edges", "generate_ms", "persist_ms", "rss_after_seed_bytes"] },
  { key: "startup_load", label: "startup load into cache (logged)", extra: ["wall_to_live_ms", "rss_after_load_bytes"] },
  { key: "rest_health", label: "REST health", extra: ["bytes"] },
  { key: "ws_list", label: "WS list, streamed (control)", extra: ["bytes", "events", "ttfb_ms"] },
  { key: "rest_list", label: "REST list (all tickets)", extra: ["bytes", "events", "ttfb_ms"] },
  { key: "rest_get", label: "REST get", extra: ["bytes"] },
  { key: "rest_ready_set", label: "REST ready_set", extra: ["bytes", "events"] },
  { key: "rest_blockers", label: "REST blockers", extra: ["count", "bytes"] },
  { key: "create_watch_push", label: "create → watch push (SSE)", extra: ["create_ack_ms"] },
  { key: "app_load_live", label: "browser: app load → live", extra: [] },
  { key: "nav_groups_visible", label: "browser: nav shortcodes visible", extra: ["groups"] },
  { key: "board_render", label: "browser: board render (all rows)", extra: ["dom_nodes"] },
  { key: "nav_expand_largest", label: "browser: expand largest nav group", extra: ["leaves", "dom_nodes"] },
  { key: "ticket_page_from_board", label: "browser: open ticket page (from board)", extra: [] },
  { key: "ticket_page_cold", label: "browser: ticket page cold deep link", extra: [] },
  { key: "live_create_appears", label: "browser: curl-created ticket appears live", extra: ["create_ms"] },
];

function cell(s) {
  if (!s || s.failure && !s.ok) return `**FAIL** (${(s?.failure ?? "not run").replace(/\|/g, "/")})`;
  let t = fmtMs(s.median_ms);
  if (s.ok < s.runs) t += ` (${s.runs - s.ok}/${s.runs} failed)`;
  return t;
}

function detail(key, s) {
  const bits = [];
  if (s.runs > 1 && s.ok > 0) bits.push(`min ${fmtMs(s.min_ms)} · max ${fmtMs(s.max_ms)}`);
  if (s.bytes != null) bits.push(`payload ${fmtBytes(s.bytes)}`);
  if (s.events != null) bits.push(`${fmtInt(s.events)} events`);
  if (s.ttfb_ms != null) bits.push(`first byte ${fmtMs(s.ttfb_ms)}`);
  if (s.epics != null) bits.push(`${s.epics} epics, ${fmtInt(s.edges)} edges`);
  if (s.generate_ms != null) bits.push(`generate ${fmtMs(s.generate_ms)} + persist ${fmtMs(s.persist_ms)}`);
  if (s.wall_to_live_ms != null) bits.push(`wall to liveness ${fmtMs(s.wall_to_live_ms)}`);
  if (s.rss_after_load_bytes != null) bits.push(`RSS after load ${fmtBytes(s.rss_after_load_bytes)}`);
  if (s.rss_after_seed_bytes != null) bits.push(`RSS after seed ${fmtBytes(s.rss_after_seed_bytes)}`);
  if (s.create_ack_ms != null) bits.push(`create ack ${fmtMs(s.create_ack_ms)}`);
  if (s.create_ms != null) bits.push(`create call ${fmtMs(s.create_ms)}`);
  if (s.groups != null) bits.push(`${s.groups} groups`);
  if (s.leaves != null) bits.push(`${fmtInt(s.leaves)} leaves`);
  if (s.dom_nodes != null) bits.push(`${fmtInt(Math.round(s.dom_nodes))} DOM nodes`);
  if (s.count != null) bits.push(`${s.count} blockers`);
  if (s.failure && s.ok) bits.push(`failures: ${s.failure}`);
  return bits.join("; ");
}

function ratioWord(a, b) {
  // Per ×10 in N: how did the median move?
  if (a == null || b == null || a <= 0) return "—";
  const r = b / a;
  if (r < 2) return `×${r.toFixed(1)} (sub-linear)`;
  if (r < 6) return `×${r.toFixed(1)}`;
  if (r < 15) return `×${r.toFixed(0)} (~linear)`;
  return `×${r.toFixed(0)} (super-linear)`;
}

function writeReport(opts, all, specs, notesPath) {
  const ms = all.sort((a, b) => a.magnitude - b.magnitude);
  const L = [];
  L.push(`# MAGNITUDE — how menger degrades from 10² to 10⁶ tickets`);
  L.push("");
  L.push(`Report of the PERF-3 ladder, generated ${new Date().toISOString()} by \`perf/magnitude.mjs\` (raw JSON, screenshots: \`${path.relative(path.join(REPO, "plans", "PERF", "artifacts"), opts.out)}/\`). Tables are regenerated from the raw files; the analysis sections at the end are authored in \`perf/notes.md\` and included verbatim.`);
  L.push("");
  L.push(`## Machine`);
  L.push("");
  L.push(`| | |`);
  L.push(`|---|---|`);
  L.push(`| CPU | ${specs.cpu} (${specs.cores} threads) |`);
  L.push(`| RAM | ${fmtBytes(specs.mem_total_bytes)} total, ${fmtBytes(specs.mem_available_bytes)} available at start; swap ${fmtBytes(specs.swap_total_bytes)} |`);
  L.push(`| Disk | ${specs.disk ?? "?"} |`);
  L.push(`| OS / kernel | ${specs.os} / ${specs.kernel} |`);
  L.push(`| Server | menger ${specs.menger} @ ${specs.commit}, \`cargo build --release\`, rustc: ${specs.rustc} |`);
  L.push(`| Browser | Playwright ${specs.playwright} chromium ${specs.chromium} (headless), Node ${specs.node} |`);
  L.push(`| Temp DB | on the NVMe under \`target/perf-tmp/\` (not tmpfs); WAL + synchronous=FULL as in production |`);
  L.push("");
  L.push(`## Method`);
  L.push("");
  L.push(`Per magnitude, a fresh database seeded with \`--seed-synthetic 10^m --seed-value ${opts.seedValue}\` (PERF-2): Zipf-sized epics, every status and kind, ~25 % of tickets with 1–3 acyclic \`blocked_by\` edges mirrored in \`unlocks\`, ~500-byte markdown bodies. The seed server is stopped, the WAL checkpointed (what a clean close does), and the server restarted to measure the load; the same process then serves every REST and browser measurement. \`list\` is also streamed over the WebSocket transport (one item per notification, what \`web/index.html\` speaks) as the control for the REST gateway's collect-then-respond envelope. Runs per measurement: ${ms.map((r) => `10^${r.magnitude}: ${r.runs}`).join(", ")}; medians reported. Every measurement has a per-magnitude budget and is recorded as **FAIL** with the reason when it exceeds it. Browser timings are from navigation start (load → live, nav, board) or from the triggering action (expand, ticket page from board, live create). The server runs on ephemeral ports; the production server on 4455–4457 and its database were never touched.`);
  L.push("");

  L.push(`## The ladder at a glance (medians)`);
  L.push("");
  L.push(`| measurement | ${ms.map((r) => `10^${r.magnitude}`).join(" | ")} |`);
  L.push(`|---|${ms.map(() => "---:").join("|")}|`);
  for (const { key, label } of MEASUREMENTS) {
    L.push(`| ${label} | ${ms.map((r) => cell(r.summary?.[key])).join(" | ")} |`);
  }
  L.push(`| list payload | ${ms.map((r) => fmtBytes(r.summary?.rest_list?.bytes)).join(" | ")} |`);
  L.push(`| server RSS after load | ${ms.map((r) => fmtBytes(r.summary?.startup_load?.rss_after_load_bytes)).join(" | ")} |`);
  L.push(`| server peak RSS (run) | ${ms.map((r) => fmtBytes(r.peak_rss_bytes)).join(" | ")} |`);
  L.push(`| database file | ${ms.map((r) => fmtBytes(r.db_bytes)).join(" | ")} |`);
  L.push(`| board DOM nodes | ${ms.map((r) => fmtInt(r.summary?.board_render?.dom_nodes)).join(" | ")} |`);
  L.push(`| largest epic | ${ms.map((r) => (r.samples?.largest_epic ? `${fmtInt(r.samples.largest_epic.size)}` : "—")).join(" | ")} |`);
  L.push("");

  if (ms.length > 1) {
    L.push(`### Scaling per ×10 in N (median ratio to the previous magnitude)`);
    L.push("");
    L.push(`| measurement | ${ms.slice(1).map((r) => `→10^${r.magnitude}`).join(" | ")} | first failure |`);
    L.push(`|---|${ms.slice(1).map(() => "---:").join("|")}|---|`);
    for (const { key, label } of MEASUREMENTS) {
      const cells = ms.slice(1).map((r, i) => {
        const a = ms[i].summary?.[key], b = r.summary?.[key];
        if (!a?.ok || !b?.ok) return "—";
        return ratioWord(a.median_ms, b.median_ms);
      });
      const firstFail = ms.find((r) => !r.summary?.[key]?.ok);
      L.push(`| ${label} | ${cells.join(" | ")} | ${firstFail ? `10^${firstFail.magnitude}` : "none"} |`);
    }
    L.push("");
  }

  for (const r of ms) {
    L.push(`## 10^${r.magnitude} — ${fmtInt(r.n)} tickets`);
    L.push("");
    const lg = r.samples?.largest_epic;
    L.push(`${r.samples?.epics ?? "?"} epics, ${fmtInt(r.samples?.edges)} blocked_by edges, largest epic ${lg ? `${lg.epic} with ${fmtInt(lg.size)} tickets` : "unknown"}; database ${fmtBytes(r.db_bytes)}; server RSS after load ${fmtBytes(r.summary?.startup_load?.rss_after_load_bytes)}, after list ${fmtBytes(r.rss_after_rest_bytes)}, after the browser ${fmtBytes(r.rss_after_browser_bytes)}, peak ${fmtBytes(r.peak_rss_bytes)}. ${r.runs} run(s) per measurement.${r.aborted ? ` **Aborted:** ${r.aborted}.` : ""}`);
    L.push("");
    L.push(`| measurement | median | status | detail |`);
    L.push(`|---|---:|---|---|`);
    for (const { key, label } of MEASUREMENTS) {
      const s = r.summary?.[key];
      if (!s) {
        L.push(`| ${label} | — | **FAIL** | not run |`);
        continue;
      }
      const status = s.ok === s.runs && s.ok > 0 ? "ok" : s.ok > 0 ? `partial (${s.ok}/${s.runs})` : "**FAIL**";
      L.push(`| ${label} | ${s.ok ? fmtMs(s.median_ms) : "—"} | ${status} | ${(s.ok ? detail(key, s) : s.failure ?? "not run").replace(/\|/g, "/")} |`);
    }
    const shots = fs.existsSync(path.join(opts.out, "shots")) ? fs.readdirSync(path.join(opts.out, "shots")).filter((f) => f.startsWith(`1e${r.magnitude}-`)).sort() : [];
    if (shots.length) {
      L.push("");
      L.push(`Screenshots: ${shots.map((f) => `[${f}](${path.relative(path.join(REPO, "plans", "PERF", "artifacts"), path.join(opts.out, "shots", f))})`).join(", ")}`);
    }
    if (r.notes?.length) {
      L.push("");
      L.push(`Notes: ${r.notes.map((n) => n.replace(/\|/g, "/")).join(" · ")}`);
    }
    L.push("");
  }

  if (notesPath && fs.existsSync(notesPath)) {
    L.push(fs.readFileSync(notesPath, "utf8").trim());
    L.push("");
  } else {
    L.push(`## Degradation curve`);
    L.push("");
    L.push(`_(perf/notes.md not found — the analysis is written after the numbers are in.)_`);
    L.push("");
  }
  return L.join("\n");
}

// ---------------------------------------------------------------------
// Main

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  fs.mkdirSync(opts.tmp, { recursive: true });
  fs.mkdirSync(path.join(opts.out, "raw"), { recursive: true });

  // Refuse to point at anything that looks like the production database.
  const prod = path.join(os.homedir(), ".local", "share", "menger");
  if (path.resolve(opts.tmp).startsWith(prod)) throw new Error(`--tmp must not be under ${prod}`);

  if (opts.reportOnly) {
    const machine = path.join(opts.out, "raw", "machine.json");
    if (!fs.existsSync(machine)) throw new Error(`--report-only: no raw run under ${opts.out}`);
    const { specs } = JSON.parse(fs.readFileSync(machine, "utf8"));
    const all = fs.readdirSync(path.join(opts.out, "raw")).filter((f) => /^1e\d\.json$/.test(f)).map((f) => JSON.parse(fs.readFileSync(path.join(opts.out, "raw", f), "utf8")));
    const reportPath = opts.smoke ? path.join(opts.out, `MAGNITUDE-${opts.date}.md`) : path.join(REPO, "plans", "PERF", "artifacts", `MAGNITUDE-${opts.date}.md`);
    fs.writeFileSync(reportPath, writeReport(opts, all, specs, path.join(HERE, "notes.md")));
    console.log(`report regenerated from ${all.length} raw file(s): ${reportPath}`);
    return;
  }

  if (opts.build) {
    console.log("building the server (cargo build --release)…");
    const b = spawnSync("cargo", ["build", "--release"], { cwd: REPO, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build --release failed");
  }
  if (!fs.existsSync(BIN)) throw new Error(`missing ${BIN}`);
  const dist = path.join(REPO, "frontend", "dist", "index.html");
  if (!fs.existsSync(dist)) {
    console.log("frontend/dist missing — building it once (trunk build --release)…");
    const t = spawnSync("trunk", ["build", "--release"], { cwd: path.join(REPO, "frontend"), stdio: "inherit" });
    if (t.status !== 0 || !fs.existsSync(dist)) throw new Error("frontend build failed; run `cd frontend && trunk build --release` first");
  }

  const browser = await chromium.launch({ headless: !opts.headed });
  const specs = machineSpecs(browser.version());
  console.log(`menger ${specs.menger}@${specs.commit} · ${specs.cpu} · ${fmtBytes(specs.mem_total_bytes)} RAM (${fmtBytes(specs.mem_available_bytes)} available) · chromium ${specs.chromium} · node ${specs.node}`);
  console.log(`magnitudes: ${opts.magnitudes.map((m) => `10^${m}`).join(", ")} · out: ${opts.out} · tmp: ${opts.tmp}`);
  if (opts.magnitudes.includes(6) && specs.mem_available_bytes != null && specs.mem_available_bytes < 8 * 1024 ** 3) {
    console.warn(`WARNING: only ${fmtBytes(specs.mem_available_bytes)} available; 10^6 wants ~8 GiB headroom (server cache + list serialization + browser)`);
  }
  fs.writeFileSync(path.join(opts.out, "raw", "machine.json"), JSON.stringify({ specs, options: opts, argv: process.argv.slice(2) }, null, 2));

  const results = [];
  try {
    for (const m of opts.magnitudes) {
      const r = await runMagnitude(m, opts, browser);
      results.push(r);
      fs.writeFileSync(path.join(opts.out, "raw", `1e${m}.json`), JSON.stringify(r, null, 2));
      console.log(`[1e${m}] done: ${MEASUREMENTS.map((x) => `${x.key}=${r.summary[x.key].ok ? fmtMs(r.summary[x.key].median_ms) : "FAIL"}`).join(" ")}`);
    }
  } finally {
    await browser.close().catch(() => {});
  }

  // Report from every raw file present (a single-magnitude re-run merges).
  const all = fs.readdirSync(path.join(opts.out, "raw")).filter((f) => /^1e\d\.json$/.test(f)).map((f) => JSON.parse(fs.readFileSync(path.join(opts.out, "raw", f), "utf8")));
  const reportPath = opts.smoke ? path.join(opts.out, `MAGNITUDE-${opts.date}.md`) : path.join(REPO, "plans", "PERF", "artifacts", `MAGNITUDE-${opts.date}.md`);
  fs.writeFileSync(reportPath, writeReport(opts, all, specs, path.join(HERE, "notes.md")));
  fs.writeFileSync(path.join(opts.out, "raw", "results.json"), JSON.stringify({ specs, results: all }, null, 2));
  console.log(`report: ${reportPath}`);

  // The smoke run's own pass/fail: every measurement at 10^2 must be ok.
  if (opts.smoke) {
    const bad = MEASUREMENTS.filter((x) => !results[0]?.summary?.[x.key]?.ok || results[0].summary[x.key].ok < results[0].summary[x.key].runs);
    if (bad.length) {
      console.error(`SMOKE FAILED: ${bad.map((x) => `${x.key} (${results[0]?.summary?.[x.key]?.failure ?? "missing"})`).join(", ")}`);
      process.exit(1);
    }
    console.log("SMOKE OK: every measurement at 10^2 succeeded");
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
