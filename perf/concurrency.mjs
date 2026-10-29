#!/usr/bin/env node
// PERF-12: the concurrency ladder — menger's memory and CPU at 1, 10, …
// 10^n concurrent signed-in WebSockets, idle, under load and after.
//
// This ladder grows the connections, not the store (V2-9: on v2, whose
// one watch is the tickets service's). It starts ONE release `menger` on
// kernel-chosen ports with a temp database (`--auth-provider key --no-rest`,
// a key of its own as `--admin`, ACCESS-52; --tickets synthetic tickets),
// with ACCESS-29's connection caps raised on that throwaway alone, and climbs
// the rungs 10^r in order. At each rung, n sockets — spread over forked
// client processes (concurrency-worker.mjs), over 127.0.0.x source
// addresses and over several keys, each key granted viewer on `*` — and,
// sampling the server's /proc every --sample-ms throughout:
//
//   idle   n sockets open, each holding tickets.watch with revisions, for
//          --idle-secs;
//   load   a fixed writer (status changes) fanned out to every watch, plus a read mix
//          per socket every --read-every-secs, for --load-secs; each write
//          timed from its send to its arrival on every socket;
//   after  every socket closed, for --after-secs: the memory floor (a leak
//          is a floor that rises rung to rung).
//
// Then the report (markdown), the curves (SVG) and the raw JSON, in --out.
// It stops at the rung the box cannot hold, and says why: too little memory
// for the rung by the last rung's cost per socket, too few file
// descriptors, sockets refused or lost, the server gone, or writes that did
// not reach every watcher within --grace-secs.
//
//   node perf/concurrency.mjs --rungs 0..4         # 1 … 10^4 sockets
//   node perf/concurrency.mjs --smoke              # rungs 0..1, short phases → perf/out/smoke-concurrency; exit 1 unless clean
//   node perf/concurrency.mjs --rungs 0..3 --idle-secs 30 --load-secs 60
//
// Options: --rungs A..B | a,b,c (exponents; default 0..4) · --idle-secs N
// (20) · --load-secs N (30) · --after-secs N (20) · --sample-ms N (250) ·
// --keys N (8) · --workers N (default: one per 250 sockets, 1..cpus/2) ·
// --sockets-per-ip N (500) · --inflight N (opens in flight per worker, 64) ·
// --status-rate R (status changes/s, 4) · --read-every-secs S (10; 0 = no reads) · --tickets N
// (seeded, 1000) · --grace-secs N (10) · --max-p99-ms N (stop past it,
// 30000) · --bin PATH (default $CARGO_TARGET_DIR/release/menger) · --build
// (cargo build --release first) · --out DIR · --tmp DIR · --keep-tmp.
//
// The production server (4455–4458), test (4450–4453) and
// ~/.local/share/menger are never touched: ports come from the kernel and
// are refused in that range, and --tmp under the data dir is refused.
// Node ≥ 22, its standard library and deploy/menger-call.mjs only.

import { fork, spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { connect } from "../deploy/menger-call.mjs";
import { makeKey, mintSession } from "../scripts/throwaway-login.mjs";

// Math.max(...xs) overflows the stack past ~10^5 elements (10^4 sockets).
const maxOf = (xs) => xs.reduce((m, x) => (x > m ? x : m), -Infinity);
const minOf = (xs) => xs.reduce((m, x) => (x < m ? x : m), Infinity);

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..");
const clock = () => performance.timeOrigin + performance.now();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const CLK_TCK = Number(spawnSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).stdout?.trim()) || 100;

// ---------------------------------------------------------------------
// Options

function parseArgs(argv) {
  const o = {
    rungs: [0, 1, 2, 3, 4], idleSecs: 20, loadSecs: 30, afterSecs: 20, sampleMs: 250, keys: 8, workers: null,
    socketsPerIp: 500, inflight: 64, statusRate: 4, readEverySecs: 10, tickets: 1000,
    graceSecs: 10, maxP99Ms: 30000, bin: null, build: false, out: null, tmp: null, keepTmp: false, smoke: false,
  };
  const num = (v, a) => { const n = Number(v); if (!Number.isFinite(n)) throw new Error(`${a} wants a number, not ${v}`); return n; };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => { const v = argv[++i]; if (v === undefined) throw new Error(`${a} needs a value`); return v; };
    const flags = {
      "--idle-secs": "idleSecs", "--load-secs": "loadSecs", "--after-secs": "afterSecs", "--sample-ms": "sampleMs", "--keys": "keys",
      "--workers": "workers", "--sockets-per-ip": "socketsPerIp", "--inflight": "inflight", "--status-rate": "statusRate",
      "--read-every-secs": "readEverySecs", "--tickets": "tickets",
      "--grace-secs": "graceSecs", "--max-p99-ms": "maxP99Ms",
    };
    if (flags[a]) o[flags[a]] = num(next(), a);
    else if (a === "--rungs") {
      const v = next();
      const m = /^(\d+)\.\.(\d+)$/.exec(v);
      o.rungs = m ? Array.from({ length: Number(m[2]) - Number(m[1]) + 1 }, (_, k) => Number(m[1]) + k) : v.split(",").map((s) => num(s, a));
    } else if (a === "--bin") o.bin = path.resolve(next());
    else if (a === "--build") o.build = true;
    else if (a === "--out") o.out = path.resolve(next());
    else if (a === "--tmp") o.tmp = path.resolve(next());
    else if (a === "--keep-tmp") o.keepTmp = true;
    else if (a === "--smoke") o.smoke = true;
    else if (a === "-h" || a === "--help") {
      console.log(fs.readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 46).map((l) => l.replace(/^\/\/ ?/, "")).join("\n"));
      process.exit(0);
    } else throw new Error(`unknown option ${a} (--help)`);
  }
  if (o.smoke) {
    if (!argv.includes("--rungs")) o.rungs = [0, 1];
    if (!argv.includes("--idle-secs")) o.idleSecs = 3;
    if (!argv.includes("--load-secs")) o.loadSecs = 6;
    if (!argv.includes("--after-secs")) o.afterSecs = 3;
    if (!argv.includes("--read-every-secs")) o.readEverySecs = 2;
    if (!argv.includes("--tickets")) o.tickets = 200;
    o.out = o.out ?? path.join(HERE, "out", "smoke-concurrency");
  }
  o.out = o.out ?? path.join(HERE, "out", "concurrency");
  o.bin = o.bin ?? path.join(process.env.CARGO_TARGET_DIR || path.join(REPO, "target"), "release", "menger");
  return o;
}

// ---------------------------------------------------------------------
// Small things

const median = (xs) => pct(xs, 50);
function pct(xs, p) {
  if (!xs.length) return null;
  const s = Float64Array.from(xs).sort();
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1))];
}
const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);
const MiB = 1024 * 1024;
const f1 = (x) => (x == null || !Number.isFinite(x) ? "—" : x.toFixed(1));
const f2 = (x) => (x == null || !Number.isFinite(x) ? "—" : x.toFixed(2));
const f3 = (x) => (x == null || !Number.isFinite(x) ? "—" : x.toFixed(3));
const fms = (ms) => (ms == null ? "—" : ms < 10 ? `${ms.toFixed(1)} ms` : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`);
const fint = (n) => (n == null ? "—" : Number(n).toLocaleString("en-US"));
const sup = (r) => (r === 0 ? "1" : r === 1 ? "10" : `10${String(r).split("").map((d) => "⁰¹²³⁴⁵⁶⁷⁸⁹"[d]).join("")}`);

function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on("error", reject);
    srv.listen(0, "127.0.0.1", () => { const { port } = srv.address(); srv.close(() => resolve(port)); });
  });
}
async function safePort() {
  for (;;) {
    const p = await freePort();
    if (p < 4440 || p > 4479) return p; // never near prod's or test's ports
  }
}
function tcpUp(port) {
  return new Promise((resolve) => {
    const s = net.connect({ port, host: "127.0.0.1" });
    s.once("connect", () => { s.destroy(); resolve(true); });
    s.once("error", () => resolve(false));
  });
}

/** One process's counters from /proc: RSS, peak RSS (VmHWM), CPU seconds,
 *  threads, open descriptors (when `fds`). */
function procSample(pid, fds = false) {
  try {
    const status = fs.readFileSync(`/proc/${pid}/status`, "utf8");
    const kb = (k) => { const m = status.match(new RegExp(`^${k}:\\s+(\\d+) kB`, "m")); return m ? Number(m[1]) * 1024 : null; };
    const stat = fs.readFileSync(`/proc/${pid}/stat`, "utf8");
    const f = stat.slice(stat.lastIndexOf(")") + 2).split(" "); // fields from 3 (state)
    const cpu = (Number(f[11]) + Number(f[12])) / CLK_TCK;      // utime (14) + stime (15)
    const threads = Number(/^Threads:\s+(\d+)/m.exec(status)?.[1]);
    let nfd = null;
    if (fds) { try { nfd = fs.readdirSync(`/proc/${pid}/fd`).length; } catch { /* gone */ } }
    return { rss: kb("VmRSS"), hwm: kb("VmHWM"), cpu, threads, fds: nfd };
  } catch {
    return null;
  }
}
/** Resets a process's VmHWM to its RSS now (proc(5): clear_refs 5), so a
 *  rung's peak is its own; false when the kernel will not. */
function resetPeak(pid) {
  try { fs.writeFileSync(`/proc/${pid}/clear_refs`, "5"); return true; } catch { return false; }
}
function memAvailable() {
  const m = /^MemAvailable:\s+(\d+) kB/m.exec(fs.readFileSync("/proc/meminfo", "utf8"));
  return m ? Number(m[1]) * 1024 : null;
}
const loadavg = () => fs.readFileSync("/proc/loadavg", "utf8").split(" ").slice(0, 3).map(Number);
function fdLimit(pid = "self") {
  const m = /^Max open files\s+(\d+|unlimited)\s+(\d+|unlimited)/m.exec(fs.readFileSync(`/proc/${pid}/limits`, "utf8"));
  const n = (v) => (v === "unlimited" ? Infinity : Number(v));
  return m ? { soft: n(m[1]), hard: n(m[2]) } : null;
}

function machine(o) {
  const first = (cmd, args) => { const r = spawnSync(cmd, args, { encoding: "utf8" }); return r.status === 0 ? r.stdout.trim() : null; };
  const cpus = os.cpus();
  return {
    host: os.hostname(), kernel: os.release(), cpu: cpus[0]?.model, cpus: cpus.length,
    mem_total_bytes: os.totalmem(), mem_available_bytes: memAvailable(), node: process.version,
    menger_bin: o.bin, menger_sha256: first("sha256sum", [o.bin])?.split(" ")[0]?.slice(0, 12),
    menger_bin_mtime: (() => { try { return fs.statSync(o.bin).mtime.toISOString(); } catch { return null; } })(),
    commit: first("git", ["-C", REPO, "rev-parse", "--short", "HEAD"]),
    fd_limit: fdLimit(), loadavg_start: loadavg(), clk_tck: CLK_TCK,
    port_range: fs.readFileSync("/proc/sys/net/ipv4/ip_local_port_range", "utf8").trim().split(/\s+/).map(Number),
  };
}

// ---------------------------------------------------------------------
// The server: one for the whole ladder, so its floor is comparable rung to rung

async function startServer(o, tmp, caps, operator) {
  const ws = await safePort();
  const shell = await safePort();
  const db = path.join(tmp, "concurrency.db");
  const args = [
    "--port", String(ws), "--shell-port", String(shell), "--no-rest",
    "--auth-provider", "key", "--admin", operator.principal, "--db-path", db,
    "--seed-synthetic", String(o.tickets),
    "--max-connections", String(caps.max_connections),
    "--max-connections-per-ip", String(caps.max_connections_per_ip),
    "--max-connections-per-principal", String(caps.max_connections_per_principal),
    "--idle-timeout-secs", String(caps.idle_timeout_secs),
  ];
  const env = { ...process.env, PLEXUS_NO_REGISTRY: "1", RUST_LOG: "warn" };
  for (const k of Object.keys(env)) if (k.startsWith("MENGER_")) delete env[k];
  // The descriptors the top rung needs: raised for the server alone when
  // this shell's limit is short of them (prlimit, within the hard limit).
  const need = caps.fds_needed;
  const lim = fdLimit();
  let cmd = o.bin, argv = args, fdNote = `inherited soft ${lim?.soft}`;
  if (lim && lim.soft < need) {
    if (lim.hard < need) throw new Error(`the top rung needs ~${need} descriptors and the hard limit is ${lim.hard}: raise it (limits.conf / systemd LimitNOFILE) or lower --rungs`);
    cmd = "prlimit"; argv = [`--nofile=${need}:${lim.hard === Infinity ? need : lim.hard}`, o.bin, ...args];
    fdNote = `raised to ${need} with prlimit`;
  }
  const child = spawn(cmd, argv, { env, stdio: ["ignore", "ignore", "pipe"] });
  let stderr = "";
  child.stderr.on("data", (d) => (stderr = (stderr + d).slice(-20000)));
  const exited = new Promise((r) => child.on("exit", (code, sig) => r({ code, sig })));
  const srv = { child, ws, shell, db, exited, stderr: () => stderr, fdNote, alive: () => child.exitCode === null && child.signalCode === null };
  const t0 = clock();
  for (;;) {
    if (!srv.alive()) throw new Error(`menger exited at startup: ${stderr.trim().split("\n").slice(-3).join(" | ")}`);
    if ((await tcpUp(ws)) && (await tcpUp(shell))) {
      try { srv.opToken = (await mintSession(`ws://127.0.0.1:${ws}`, operator.keyFile, "perf-writer")).token; break; } catch { /* not yet */ }
    }
    if (clock() - t0 > 120_000) throw new Error("menger did not come up within 120 s");
    await sleep(100);
  }
  srv.startup_ms = clock() - t0;
  srv.pid = child.pid; // prlimit execs menger, so the pid is menger's
  srv.fd_limit = fdLimit(srv.pid);
  return srv;
}

/** A signed-in socket of the parent's own (the writer, the setup). */
class Rpc {
  constructor(ws) { this.ws = ws; this.id = 1; this.calls = new Map(); this.subs = new Map(); ws.onmessage = (t) => this.on(t); ws.onclose = () => { for (const c of this.calls.values()) c.reject(new Error("socket closed")); }; }
  static async open(port, token) { return new Rpc(await connect(`ws://127.0.0.1:${port}`, { Authorization: `Bearer ${token}` })); }
  on(text) {
    const msg = JSON.parse(text);
    if (msg.id != null) {
      const c = this.calls.get(msg.id);
      if (!c) return;
      if (msg.error) { this.calls.delete(msg.id); return c.reject(new Error(`${c.method}: ${msg.error.message}`)); }
      this.subs.set(msg.result, c);
      return;
    }
    const c = this.subs.get(msg.params?.subscription);
    if (!c) return;
    const item = msg.params.result;
    if (item?.type === "data") c.items.push(item.content);
    else if (item?.type === "error") { c.items.push({ type: "error", ...item }); }
    if (item?.type === "done" || item?.type === "error") { this.subs.delete(msg.params.subscription); this.calls.delete(c.id); c.resolve(c.items); }
  }
  call(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = this.id++;
      this.calls.set(id, { id, method, items: [], resolve, reject, t: clock() });
      this.ws.send(JSON.stringify({ jsonrpc: "2.0", id, method: "menger.call", params: { method, params } }));
    });
  }
  async must(method, params) {
    const items = await this.call(method, params);
    const bad = items.find((i) => i?.type === "error" || i?.type === "refused" || i?.type === "not_found");
    if (bad) throw new Error(`${method}: ${JSON.stringify(bad).slice(0, 300)}`);
    return items;
  }
  close() { this.ws.close(); }
}

// ---------------------------------------------------------------------
// The clients

class Worker {
  constructor(k) {
    this.k = k;
    this.proc = fork(path.join(HERE, "concurrency-worker.mjs"), [], { stdio: ["ignore", "inherit", "inherit", "ipc"] });
    this.waiters = [];
    this.proc.on("message", (m) => {
      if (m.type === "error") console.error(`  worker ${k}: ${m.message}`);
      const w = this.waiters.findIndex((x) => x.type === m.type);
      if (w >= 0) this.waiters.splice(w, 1)[0].resolve(m);
    });
    this.proc.on("exit", (code) => { for (const x of this.waiters.splice(0)) x.reject(new Error(`worker ${k} exited (${code})`)); });
  }
  get pid() { return this.proc.pid; }
  ask(cmd, type) { return new Promise((resolve, reject) => { this.waiters.push({ type, resolve, reject }); this.proc.send(cmd); }); }
  tell(cmd) { try { this.proc.send(cmd); } catch { /* gone */ } }
  kill() { try { this.proc.disconnect(); } catch { /* gone */ } this.proc.kill(); }
}

const addrOf = (j) => `127.0.${1 + Math.floor(j / 250)}.${1 + (j % 250)}`;

// ---------------------------------------------------------------------
// Sampling

class Sampler {
  constructor(srv, clients, ms) { this.srv = srv; this.clients = clients; this.ms = ms; this.samples = []; this.phase = "start"; this.rung = null; this.t0 = clock(); }
  start() { this.take(); this.timer = setInterval(() => this.take(), this.ms); }
  stop() { clearInterval(this.timer); }
  take() {
    const t = clock();
    const s = procSample(this.srv.pid, true);
    const cl = this.clients().map((w) => procSample(w.pid)).filter(Boolean);
    const me = process.memoryUsage().rss, mc = process.cpuUsage();
    this.samples.push({
      t: (t - this.t0) / 1000, rung: this.rung, phase: this.phase,
      rss: s?.rss ?? null, hwm: s?.hwm ?? null, cpu: s?.cpu ?? null, threads: s?.threads ?? null, fds: s?.fds ?? null,
      client_rss: cl.reduce((a, c) => a + c.rss, 0) + me, client_cpu: cl.reduce((a, c) => a + c.cpu, 0) + (mc.user + mc.system) / 1e6,
    });
  }
  mark(rung, phase) { this.take(); this.rung = rung; this.phase = phase; this.take(); }
  window(rung, phase, fromFrac = 0) {
    const xs = this.samples.filter((s) => s.rung === rung && s.phase === phase && s.rss != null);
    if (xs.length < 2) return null;
    const a = xs[Math.floor((xs.length - 1) * fromFrac)], b = xs[xs.length - 1];
    const tail = xs.slice(Math.floor((xs.length - 1) * fromFrac));
    const dt = b.t - a.t;
    return {
      secs: dt, samples: xs.length,
      rss_mean: mean(tail.map((s) => s.rss)), rss_max: maxOf(xs.map((s) => s.rss)), rss_end: b.rss,
      cpu_rate: dt > 0 ? (b.cpu - a.cpu) / dt : null,
      client_cpu_rate: dt > 0 ? (b.client_cpu - a.client_cpu) / dt : null,
      client_rss_max: maxOf(xs.map((s) => s.client_rss)),
      threads_max: maxOf(xs.map((s) => s.threads ?? 0)), fds_max: maxOf(xs.map((s) => s.fds ?? 0)),
    };
  }
}

// ---------------------------------------------------------------------
// One rung

async function runRung(r, ctx) {
  const { o, srv, sampler, writer, pool, tokensFile, plan } = ctx;
  const n = 10 ** r;
  const workersN = Math.max(1, Math.min(o.workers ?? Math.max(1, Math.floor(os.cpus().length / 2)), Math.ceil(n / (o.workers ? 1 : 250))));
  const res = { rung: r, n, workers: workersN, loadavg_before: loadavg(), mem_available_before: memAvailable() };
  console.log(`\n[rung ${r}] ${fint(n)} socket(s) over ${workersN} worker(s) · load average ${res.loadavg_before.join(" ")}`);

  // Before: the floor this rung starts from; the rung's own peak.
  sampler.mark(r, "before");
  res.peak_reset = resetPeak(srv.pid);
  await sleep(Math.max(1000, o.sampleMs * 4));
  const before = procSample(srv.pid);
  res.before = { rss: before?.rss, cpu: before?.cpu };

  // Open.
  const workers = Array.from({ length: workersN }, (_, k) => new Worker(k));
  ctx.workers = workers;
  const watches = [
    { method: "tickets.watch", params: { revisions: true } },
  ];
  sampler.mark(r, "open");
  const t0 = clock();
  const shares = workers.map(() => []);
  for (let i = 0; i < n; i++) shares[i % workersN].push({ i, addr: addrOf(Math.floor(i / o.socketsPerIp)), key: i % o.keys });
  const opened = await Promise.all(workers.map((w, k) => w.ask({
    cmd: "open", url: `ws://127.0.0.1:${srv.ws}`, tokensFile, sockets: shares[k], watches, inflight: o.inflight, timeoutMs: 60_000,
  }, "opened")));
  res.open = {
    secs: (clock() - t0) / 1000,
    ok: opened.reduce((a, x) => a + x.ok, 0), failed: opened.reduce((a, x) => a + x.failed, 0),
    ms_p50: median(opened.flatMap((x) => x.open_ms)), ms_p99: pct(opened.flatMap((x) => x.open_ms), 99),
    errors: Object.assign({}, ...opened.map((x) => x.errors)),
    source_addresses: Math.ceil(n / o.socketsPerIp), keys: Math.min(n, o.keys),
  };
  console.log(`  opened ${fint(res.open.ok)}/${fint(n)} in ${res.open.secs.toFixed(1)} s (p50 ${fms(res.open.ms_p50)}, p99 ${fms(res.open.ms_p99)})${res.open.failed ? ` — ${res.open.failed} FAILED: ${JSON.stringify(res.open.errors)}` : ""}`);

  // Idle.
  sampler.mark(r, "idle");
  await sleep(o.idleSecs * 1000);
  res.idle = sampler.window(r, "idle", 0.5);

  // Load.
  sampler.mark(r, "load");
  for (const w of workers) w.tell({ cmd: "load", readEverySecs: o.readEverySecs, reads: plan.reads });
  const writes = [];
  const occurrences = new Map();
  let seq = 0;
  const writeOnce = async (kind) => {
    seq++;
    const tk = pool[seq % pool.length];
    tk.status = tk.status === "Coding" ? "In Review" : "Coding";
    const method = "tickets.set_status", params = { id: tk.id, status: tk.status }, key = `status:${tk.id}>${tk.status}`;
    const occurrence = (occurrences.get(key) ?? 0) + 1;
    occurrences.set(key, occurrence);
    const t = clock();
    for (const w of workers) w.tell({ cmd: "write", key, occurrence, t, kind });
    const p = writer.call(method, params);
    const wr = { kind, key, occurrence, t, ack_ms: null, error: null };
    writes.push(wr);
    try {
      const items = await p;
      wr.ack_ms = clock() - t;
      const bad = items.find((i) => i?.type === "error" || i?.type === "refused" || i?.type === "not_found");
      if (bad) wr.error = JSON.stringify(bad).slice(0, 200);
    } catch (e) { wr.error = e.message; }
  };
  // The announcement goes before the send, so a worker never hears a write
  // it was not told of; the clock is read just before both.
  const timers = [["status", o.statusRate]]
    .filter(([, rate]) => rate > 0)
    .map(([kind, rate]) => setInterval(() => writeOnce(kind), 1000 / rate));
  await sleep(o.loadSecs * 1000);
  for (const t of timers) clearInterval(t);
  res.load = sampler.window(r, "load", 0.25);
  sampler.mark(r, "drain");
  const stats = await Promise.all(workers.map((w) => w.ask({ cmd: "drain", graceMs: o.graceSecs * 1000, watchKinds: ["tickets"] }, "loadStats")));

  // Latency: per delivery, and per write until it reached every watcher.
  const live = stats.reduce((a, s) => a + s.live, 0);
  const all = stats.flatMap((s) => s.deliveries);
  const byKind = {};
  for (const kind of ["status"]) {
    const ds = all.filter((d) => d.kind === kind).map((d) => d.ms);
    const ws = writes.filter((w) => w.kind === kind && !w.error);
    const everyMs = [], short = [];
    for (const w of ws) {
      const wkey = `${w.key}#${w.occurrence}`;
      const per = stats.map((s) => s.per_write[wkey]).filter(Boolean);
      const got = per.reduce((a, p) => a + p.n, 0);
      if (got >= live) everyMs.push(maxOf(per.map((p) => p.max ?? 0)));
      else short.push({ key: wkey, got, of: live });
    }
    byKind[kind] = {
      writes: writes.filter((w) => w.kind === kind).length, refused: writes.filter((w) => w.kind === kind && w.error).length,
      ack_p50: median(ws.map((w) => w.ack_ms).filter((x) => x != null)), ack_p99: pct(ws.map((w) => w.ack_ms).filter((x) => x != null), 99),
      delivery_p50: median(ds), delivery_p99: pct(ds, 99), delivery_max: ds.length ? maxOf(ds) : null,
      every_p50: median(everyMs), every_p99: pct(everyMs, 99), every_max: everyMs.length ? maxOf(everyMs) : null,
      reached_every: everyMs.length, short: short.length, short_examples: short.slice(0, 5),
      errors: [...new Set(writes.filter((w) => w.kind === kind && w.error).map((w) => w.error))].slice(0, 3),
    };
  }
  const allEvery = [];
  for (const k of Object.keys(byKind)) allEvery.push(byKind[k].every_p99);
  const readMs = stats.flatMap((s) => s.reads.ms);
  const byMethod = {};
  for (const s of stats) for (const [m, v] of Object.entries(s.reads.by_method)) { const a = (byMethod[m] ??= []); for (const x of v.ms) a.push(x); }
  const events = {};
  for (const s of stats) for (const [k, v] of Object.entries(s.events_by_type)) events[k] = (events[k] ?? 0) + v;
  res.latency = {
    live, by_kind: byKind,
    delivery_p50: median(all.map((d) => d.ms)), delivery_p99: pct(all.map((d) => d.ms), 99),
    sampled: stats.some((s) => s.sampled), deliveries_total: stats.reduce((a, s) => a + s.deliveries_total, 0),
  };
  res.reads = {
    sent: stats.reduce((a, s) => a + s.reads.sent, 0), ok: stats.reduce((a, s) => a + s.reads.ok, 0),
    errors: stats.reduce((a, s) => a + s.reads.errors, 0), in_flight: stats.reduce((a, s) => a + s.reads.in_flight, 0),
    p50: median(readMs), p99: pct(readMs, 99),
    by_method: Object.fromEntries(Object.entries(byMethod).map(([m, xs]) => [m, { n: xs.length, p50: median(xs), p99: pct(xs, 99) }])),
    rate_per_s: o.readEverySecs > 0 ? n / o.readEverySecs : 0,
  };
  res.events_by_type = events;
  res.worker_errors = Object.assign({}, ...stats.map((s) => s.errors));
  res.closed_early = stats.reduce((a, s) => a + s.closed_early, 0);
  res.write_rate_per_s = o.statusRate;
  res.writes = writes.length;
  const peak = procSample(srv.pid);
  res.peak_rss = res.peak_reset ? peak?.hwm : maxOf(sampler.samples.filter((s) => s.rung === r && s.rss).map((s) => s.rss));
  console.log(`  idle ${f1(res.idle?.rss_mean / MiB)} MiB, ${f3(res.idle?.cpu_rate)} cpu-s/s · load ${f1(res.load?.rss_mean / MiB)} MiB, ${f3(res.load?.cpu_rate)} cpu-s/s · peak ${f1(res.peak_rss / MiB)} MiB`);
  console.log(`  status → every watcher p50 ${fms(byKind.status.every_p50)} p99 ${fms(byKind.status.every_p99)} · deliveries p50 ${fms(res.latency.delivery_p50)} p99 ${fms(res.latency.delivery_p99)} · reads ${fint(res.reads.ok)}/${fint(res.reads.sent)} p99 ${fms(res.reads.p99)}`);

  // After: every socket closed; the floor.
  sampler.mark(r, "after");
  await Promise.all(workers.map((w) => w.ask({ cmd: "close" }, "closed")));
  for (const w of workers) w.kill();
  ctx.workers = [];
  await sleep(o.afterSecs * 1000);
  res.after = sampler.window(r, "after", 0.5);
  res.floor_rss = res.after?.rss_end ?? procSample(srv.pid)?.rss;
  res.loadavg_after = loadavg();
  console.log(`  after ${f1(res.floor_rss / MiB)} MiB (started the rung at ${f1(res.before.rss / MiB)} MiB) · load average ${res.loadavg_after.join(" ")}`);

  // Whether the box held it.
  const problems = [];
  if (!srv.alive()) problems.push(`the server exited (${JSON.stringify(await Promise.race([srv.exited, sleep(10)]))}): ${srv.stderr().trim().split("\n").slice(-2).join(" | ")}`);
  if (res.open.failed > 0) problems.push(`${res.open.failed} of ${n} sockets did not open (${Object.keys(res.open.errors).slice(0, 2).join("; ")})`);
  if (res.closed_early > 0) problems.push(`${res.closed_early} sockets were closed under load (${Object.keys(res.worker_errors).slice(0, 2).join("; ")})`);
  for (const [k, v] of Object.entries(byKind)) {
    if (v.short) problems.push(`${v.short} of ${v.writes} ${k} writes had not reached every watcher within ${o.graceSecs} s`);
    if (v.refused) problems.push(`${v.refused} ${k} writes refused: ${v.errors.join("; ")}`);
  }
  const p99 = Math.max(...Object.values(byKind).map((v) => v.every_p99 ?? 0));
  if (p99 > o.maxP99Ms) problems.push(`write → every watcher p99 ${fms(p99)} is past --max-p99-ms ${o.maxP99Ms}`);
  if (res.reads.errors) problems.push(`${res.reads.errors} reads failed`);
  if (res.events_by_type && Object.keys(res.events_by_type).some((k) => /lagged/.test(k))) problems.push(`watches lagged: ${Object.entries(res.events_by_type).filter(([k]) => /lagged/.test(k)).map(([k, v]) => `${k} ${v}`).join(", ")}`);
  res.problems = problems;
  return res;
}

// ---------------------------------------------------------------------
// The analysis

/** Least squares y = a + b·x. */
function fit(points) {
  const n = points.length;
  if (n < 2) return null;
  const mx = mean(points.map((p) => p.x)), my = mean(points.map((p) => p.y));
  const sxx = points.reduce((a, p) => a + (p.x - mx) ** 2, 0);
  if (!sxx) return null;
  const b = points.reduce((a, p) => a + (p.x - mx) * (p.y - my), 0) / sxx;
  const a = my - b * mx;
  const ss = points.reduce((s, p) => s + (p.y - my) ** 2, 0);
  const se = points.reduce((s, p) => s + (p.y - (a + b * p.x)) ** 2, 0);
  return { a, b, r2: ss ? 1 - se / ss : 1 };
}

function analyse(rungs, baseline) {
  const rows = rungs.map((r) => {
    const idleDelta = r.idle ? r.idle.rss_mean - r.before.rss : null;
    const loadDelta = r.load ? r.load.rss_mean - r.before.rss : null;
    return {
      r: r.rung, n: r.n,
      idle_rss_delta: idleDelta, load_rss_delta: loadDelta,
      mb_per_conn_idle: idleDelta != null ? idleDelta / MiB / r.n : null,
      mb_per_conn_load: loadDelta != null ? loadDelta / MiB / r.n : null,
      cpu_idle: r.idle?.cpu_rate, cpu_load: r.load?.cpu_rate,
      cpu_per_conn_idle: r.idle ? r.idle.cpu_rate / r.n : null, cpu_per_conn_load: r.load ? r.load.cpu_rate / r.n : null,
      floor_rise: r.floor_rss != null ? r.floor_rss - baseline : null,
    };
  });
  const big = rows.filter((x) => x.n >= 10);
  const fits = {
    idle_rss: fit(big.map((x) => ({ x: x.n, y: x.idle_rss_delta / MiB }))),
    load_rss: fit(big.map((x) => ({ x: x.n, y: x.load_rss_delta / MiB }))),
    idle_cpu: fit(big.map((x) => ({ x: x.n, y: x.cpu_idle }))),
    load_cpu: fit(big.map((x) => ({ x: x.n, y: x.cpu_load }))),
  };
  // Growth exponents between rungs: d log y / d log n (1 = linear).
  const exps = [];
  for (let k = 1; k < rows.length; k++) {
    const a = rows[k - 1], b = rows[k];
    const e = (ya, yb) => (ya > 0 && yb > 0 ? Math.log(yb / ya) / Math.log(b.n / a.n) : null);
    exps.push({
      from: a.n, to: b.n,
      idle_rss: e(a.idle_rss_delta, b.idle_rss_delta), load_rss: e(a.load_rss_delta, b.load_rss_delta),
      load_cpu: e(a.cpu_load, b.cpu_load),
      every_p99: e(rungs[k - 1].latency.by_kind.status.every_p99, rungs[k].latency.by_kind.status.every_p99),
    });
  }
  // Where it bends: the first step at which memory or CPU grows faster than
  // the sockets (exponent > 1.25), or the latency to every watcher at least
  // triples per ×10 (exponent > 0.5) — read only where the quantity is past
  // noise (≥ 5 MiB, ≥ 0.05 cpu-s/s, ≥ 20 ms).
  const bends = [];
  for (const [k, x] of exps.entries()) {
    const b = rows[k + 1], rb = rungs[k + 1];
    if (x.load_rss > 1.25 && b.load_rss_delta > 5 * MiB) bends.push({ at: x.to, what: `memory under load grows as n^${x.load_rss.toFixed(2)}` });
    if (x.load_cpu > 1.25 && b.cpu_load > 0.05) bends.push({ at: x.to, what: `CPU under load grows as n^${x.load_cpu.toFixed(2)}` });
    if (x.every_p99 > 0.5 && rb.latency.by_kind.status.every_p99 > 20) bends.push({ at: x.to, what: `status → every watcher p99 grows as n^${x.every_p99.toFixed(2)}` });
  }
  return { rows, fits, exps, bends };
}

// ---------------------------------------------------------------------
// The curves (SVG; light and dark from the same marks)

const PALETTE = { light: ["#2a78d6", "#eb6834", "#1baf7a", "#eda100"], dark: ["#3987e5", "#d95926", "#199e70", "#c98500"] };
const SVG_STYLE = `<style>
  svg { --bg:#fcfcfb; --ink:#0b0b0b; --ink2:#52514e; --grid:#e6e5e0; ${PALETTE.light.map((c, i) => `--s${i + 1}:${c};`).join(" ")} font: 12px system-ui, sans-serif; }
  @media (prefers-color-scheme: dark) { svg { --bg:#1a1a19; --ink:#f0efec; --ink2:#c3c2b7; --grid:#33322f; ${PALETTE.dark.map((c, i) => `--s${i + 1}:${c};`).join(" ")} } }
  .bg { fill: var(--bg); } .t { fill: var(--ink); font-weight: 600; font-size: 13px; } .l { fill: var(--ink2); } .g { stroke: var(--grid); stroke-width: 1; }
  .ph { fill: var(--ink2); opacity: .05; } .ph-load { opacity: .16; } .ph-after { opacity: .02; } .line { fill: none; stroke-width: 2; } .dot:hover { stroke: var(--ink); stroke-width: 2; }
</style>`;
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;");

/** A line chart: `series` [{name, points:[{x,y,tip}]}], one y-axis. */
function lineChart({ title, xLabel, yLabel, series, logX = false, logY = false, bands = [], w = 760, h = 320 }) {
  const m = { l: 64, r: 16, t: series.length > 1 ? 50 : 36, b: 44 };
  const pts = series.flatMap((s) => s.points).filter((p) => p.y != null && Number.isFinite(p.y) && (!logY || p.y > 0));
  if (!pts.length) return null;
  const tx = (v) => (logX ? Math.log10(v) : v), ty = (v) => (logY ? Math.log10(v) : v);
  let [x0, x1] = [minOf(pts.map((p) => tx(p.x))), maxOf(pts.map((p) => tx(p.x)))];
  let [y0, y1] = [logY ? minOf(pts.map((p) => ty(p.y))) : Math.min(0, minOf(pts.map((p) => p.y))), maxOf(pts.map((p) => ty(p.y)))];
  if (x1 === x0) { x0 -= 1; x1 += 1; }
  if (y1 === y0) { y1 = y0 + 1; }
  if (logY) { y0 = Math.floor(y0); y1 = Math.ceil(y1); } else y1 *= 1.08;
  const X = (v) => m.l + ((tx(v) - x0) / (x1 - x0)) * (w - m.l - m.r);
  const Y = (v) => h - m.b - ((ty(v) - y0) / (y1 - y0)) * (h - m.t - m.b);
  const ticks = (a, b, log) => {
    if (log) return Array.from({ length: Math.floor(b) - Math.ceil(a) + 1 }, (_, k) => 10 ** (Math.ceil(a) + k));
    const raw = (b - a) / 4, mag = 10 ** Math.floor(Math.log10(raw)), step = [1, 2, 2.5, 5, 10].map((f) => f * mag).find((x) => x >= raw);
    const out = [];
    for (let v = Math.ceil(a / step) * step; v <= b + 1e-9; v += step) out.push(+v.toPrecision(12));
    return out;
  };
  const fmt = (v) => (Math.abs(v) >= 1000 ? fint(Math.round(v)) : Math.abs(v) >= 10 ? Math.round(v) : Math.abs(v) >= 1 ? v.toFixed(1) : v === 0 ? "0" : v.toPrecision(2));
  let out = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" role="img" aria-label="${esc(title)}">${SVG_STYLE}<rect class="bg" width="${w}" height="${h}" rx="6"/>`;
  out += `<text class="t" x="${m.l}" y="20">${esc(title)}</text>`;
  for (const b of bands) out += `<rect class="ph ph-${b.phase}" x="${X(b.x0)}" y="${m.t}" width="${Math.max(0, X(b.x1) - X(b.x0))}" height="${h - m.t - m.b}"><title>${esc(b.label)}</title></rect><text class="l" x="${X(b.x0) + 3}" y="${m.t + 12}" font-size="10">${esc(b.short ?? "")}</text>`;
  for (const v of ticks(y0, y1, logY)) out += `<line class="g" x1="${m.l}" x2="${w - m.r}" y1="${Y(logY ? v : v)}" y2="${Y(v)}"/><text class="l" x="${m.l - 6}" y="${Y(v) + 4}" text-anchor="end">${fmt(v)}</text>`;
  for (const v of ticks(x0, x1, logX)) out += `<text class="l" x="${X(v)}" y="${h - m.b + 16}" text-anchor="middle">${fmt(v)}</text>`;
  out += `<text class="l" x="${(m.l + w - m.r) / 2}" y="${h - 8}" text-anchor="middle">${esc(xLabel)}</text>`;
  out += `<text class="l" transform="translate(14 ${(m.t + h - m.b) / 2}) rotate(-90)" text-anchor="middle">${esc(yLabel)}</text>`;
  series.forEach((s, i) => {
    const ps = s.points.filter((p) => p.y != null && Number.isFinite(p.y) && (!logY || p.y > 0));
    if (!ps.length) return;
    const c = `var(--s${i + 1})`;
    out += `<path class="line" stroke="${c}" d="${ps.map((p, k) => `${k ? "L" : "M"}${X(p.x).toFixed(1)},${Y(p.y).toFixed(1)}`).join("")}"/>`;
    if (s.dots !== false) for (const p of ps) out += `<circle class="dot" cx="${X(p.x).toFixed(1)}" cy="${Y(p.y).toFixed(1)}" r="4" fill="${c}" stroke="var(--bg)" stroke-width="2"><title>${esc(p.tip ?? `${s.name}: ${fmt(p.y)}`)}</title></circle>`;
  });
  if (series.length > 1) { let lx = m.l; for (const [i, s] of series.entries()) { out += `<rect x="${lx}" y="28" width="10" height="10" rx="2" fill="var(--s${i + 1})"/><text class="l" x="${lx + 14}" y="37">${esc(s.name)}</text>`; lx += 24 + s.name.length * 7; } }
  return out + "</svg>\n";
}

function curves(o, rungs, samples) {
  const files = {};
  const bands = [];
  for (const r of rungs) {
    for (const ph of ["idle", "load", "after"]) {
      const xs = samples.filter((s) => s.rung === r.rung && s.phase === ph);
      if (xs.length) bands.push({ phase: ph, x0: xs[0].t, x1: xs[xs.length - 1].t, label: `${sup(r.rung)} sockets, ${ph}`, short: ph === "idle" ? sup(r.rung) : "" });
    }
  }
  const ts = samples.filter((s) => s.rss != null);
  // CPU over time: the rate between consecutive samples.
  const cpu = [];
  for (let k = 1; k < ts.length; k++) { const dt = ts[k].t - ts[k - 1].t; if (dt > 0) cpu.push({ x: ts[k].t, y: (ts[k].cpu - ts[k - 1].cpu) / dt }); }
  files["rss-over-time.svg"] = lineChart({ title: "menger RSS over the ladder (shaded per rung: idle light · load dark · after faint)", xLabel: "seconds since start", yLabel: "RSS (MiB)", bands, series: [{ name: "RSS", dots: false, points: ts.map((s) => ({ x: s.t, y: s.rss / MiB })) }] });
  files["cpu-over-time.svg"] = lineChart({ title: "menger CPU over the ladder (CPU-seconds per wall-second)", xLabel: "seconds since start", yLabel: "CPU-s / s", bands, series: [{ name: "CPU", dots: false, points: cpu }] });
  const rr = rungs.filter((r) => r.idle && r.load);
  files["memory-per-socket.svg"] = lineChart({
    title: "Server memory added by n sockets (over the rung's starting RSS)", xLabel: "concurrent sockets (log)", yLabel: "MiB added (log)", logX: true, logY: true,
    series: [
      { name: "idle", points: rr.map((r) => ({ x: r.n, y: (r.idle.rss_mean - r.before.rss) / MiB, tip: `${fint(r.n)} sockets idle: +${f1((r.idle.rss_mean - r.before.rss) / MiB)} MiB` })) },
      { name: "under load", points: rr.map((r) => ({ x: r.n, y: (r.load.rss_mean - r.before.rss) / MiB, tip: `${fint(r.n)} sockets under load: +${f1((r.load.rss_mean - r.before.rss) / MiB)} MiB` })) },
    ],
  });
  files["cpu-per-rung.svg"] = lineChart({
    title: "Server CPU at n sockets", xLabel: "concurrent sockets (log)", yLabel: "CPU-s / s (log)", logX: true, logY: true,
    series: [
      { name: "idle", points: rr.map((r) => ({ x: r.n, y: r.idle.cpu_rate, tip: `${fint(r.n)} idle: ${f3(r.idle.cpu_rate)} cpu-s/s` })) },
      { name: "under load", points: rr.map((r) => ({ x: r.n, y: r.load.cpu_rate, tip: `${fint(r.n)} under load: ${f3(r.load.cpu_rate)} cpu-s/s` })) },
    ],
  });
  files["latency.svg"] = lineChart({
    title: "Status change → every watcher, and per delivery", xLabel: "concurrent sockets (log)", yLabel: "ms (log)", logX: true, logY: true,
    series: [
      { name: "every watcher p50", points: rr.map((r) => ({ x: r.n, y: r.latency.by_kind.status.every_p50 })) },
      { name: "every watcher p99", points: rr.map((r) => ({ x: r.n, y: r.latency.by_kind.status.every_p99 })) },
      { name: "per delivery p50", points: rr.map((r) => ({ x: r.n, y: r.latency.by_kind.status.delivery_p50 })) },
      { name: "per delivery p99", points: rr.map((r) => ({ x: r.n, y: r.latency.by_kind.status.delivery_p99 })) },
    ],
  });
  files["floor.svg"] = lineChart({
    title: "Memory floor after each rung (every socket closed)", xLabel: "rung (sockets, log)", yLabel: "RSS (MiB)", logX: true,
    series: [{ name: "floor", points: rungs.map((r) => ({ x: r.n, y: r.floor_rss / MiB, tip: `after ${fint(r.n)}: ${f1(r.floor_rss / MiB)} MiB` })) }],
  });
  for (const [k, v] of Object.entries(files)) if (!v) delete files[k];
  return files;
}

// ---------------------------------------------------------------------
// The report

function report(o, run) {
  const { spec, caps, rungs, stop, analysis, baseline, srvInfo, charts, date } = run;
  const L = [];
  L.push(`# The concurrency ladder — ${date}${o.smoke ? " (smoke)" : ""}`, "");
  L.push(`PERF-12: menger's memory and CPU at ${rungs.map((r) => sup(r.rung)).join(", ")} concurrent signed-in WebSockets — idle (each holding the app's watches), under a fixed writer plus a per-socket read mix, and after every socket closed. One server for the whole ladder, so the floor is comparable rung to rung. Raw data: \`raw/run.json\` (every sample, every rung).`, "");
  L.push("## The machine", "");
  L.push(`- ${spec.cpu} · ${spec.cpus} CPUs · ${f1(spec.mem_total_bytes / 1024 ** 3)} GiB RAM (${f1(spec.mem_available_bytes / 1024 ** 3)} GiB available at start) · kernel ${spec.kernel} · node ${spec.node}`);
  L.push(`- **Load average** at start: ${spec.loadavg_start.join(" / ")} (1 / 5 / 15 min); at the end: ${run.loadavg_end.join(" / ")}; per rung below. ${run.note ?? ""}`);
  L.push(`- menger: \`${spec.menger_bin}\` (sha256 ${spec.menger_sha256 ?? "?"}…, built ${spec.menger_bin_mtime ?? "?"}) · harness at ${spec.commit}`);
  L.push(`- Descriptors: this shell ${spec.fd_limit?.soft} soft / ${spec.fd_limit?.hard} hard; the server ${srvInfo.fdNote} (its limit ${srvInfo.fd_limit?.soft}). Ephemeral ports ${spec.port_range.join("–")} per source address.`);
  L.push("");
  L.push("## The throwaway's caps (ACCESS-29, raised here only)", "");
  L.push("| flag | default | here |", "| -- | --: | --: |");
  L.push(`| --max-connections | 1,024 | ${fint(caps.max_connections)} |`);
  L.push(`| --max-connections-per-ip | 256 | ${fint(caps.max_connections_per_ip)} |`);
  L.push(`| --max-connections-per-principal | 64 | ${fint(caps.max_connections_per_principal)} |`);
  L.push(`| --idle-timeout-secs | 900 | ${caps.idle_timeout_secs} |`);
  L.push(`| --max-calls-per-connection | 256 | 256 (1 watch + ≤ a few reads per socket) |`, "");
  L.push("## The load", "");
  L.push(`- Each socket: \`tickets.watch {revisions: true}\` — what a v2 page holds. Clients: ${o.keys} keys (viewer on \`*\`), at most ${fint(o.socketsPerIp)} sockets per 127.0.x.y source address, forked worker processes (one per 250 sockets, up to ${Math.max(1, Math.floor(os.cpus().length / 2))}).`);
  L.push(`- Phases: idle ${o.idleSecs} s · load ${o.loadSecs} s · after ${o.afterSecs} s; /proc sampled every ${o.sampleMs} ms. Idle and after are read over each phase's second half, load over its last three quarters.`);
  L.push(`- Writer (one socket, the operator): ${o.statusRate} status changes/s (Coding ⇄ In Review over ${run.poolSize} tickets — a status_changed carries the whole ticket to every watch). Reads: every socket every ${o.readEverySecs} s, cycling ${run.plan.reads.map((x) => `\`${x.method}\``).join(", ")}.`);
  L.push(`- Store: ${fint(o.tickets)} synthetic tickets (\`--seed-synthetic\`). Server up in ${fms(srvInfo.startup_ms)}; RSS before the first rung ${f1(baseline / MiB)} MiB.`, "");

  L.push("## The ladder at a glance", "");
  L.push("| sockets | opened | idle RSS | +MiB | MiB/conn idle | idle CPU | load RSS | MiB/conn load | load CPU | CPU/conn load | peak | floor after | status → every p50 / p99 | per delivery p50 / p99 | reads p50 / p99 | clients CPU / RSS |");
  L.push("| --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: |");
  for (const r of rungs) {
    const a = analysis.rows.find((x) => x.r === r.rung);
    const s = r.latency?.by_kind.status;
    L.push(`| ${fint(r.n)} | ${fint(r.open.ok)}${r.open.failed ? ` (**${r.open.failed} failed**)` : ""} | ${f1(r.idle?.rss_mean / MiB)} | ${f1(a.idle_rss_delta / MiB)} | ${f3(a.mb_per_conn_idle)} | ${f3(r.idle?.cpu_rate)} | ${f1(r.load?.rss_mean / MiB)} | ${f3(a.mb_per_conn_load)} | ${f3(r.load?.cpu_rate)} | ${a.cpu_per_conn_load != null ? a.cpu_per_conn_load.toExponential(2) : "—"} | ${f1(r.peak_rss / MiB)} | ${f1(r.floor_rss / MiB)} | ${fms(s?.every_p50)} / ${fms(s?.every_p99)} | ${fms(s?.delivery_p50)} / ${fms(s?.delivery_p99)} | ${fms(r.reads?.p50)} / ${fms(r.reads?.p99)} | ${f2(r.load?.client_cpu_rate)} / ${f1(r.load?.client_rss_max / MiB)} MiB |`);
  }
  L.push("", "RSS in MiB; CPU in CPU-seconds per wall-second of the menger process (1.0 = one core busy). +MiB, MiB/conn: over the RSS the rung started at. Peak: the rung's VmHWM (reset at the rung's start). Floor: RSS at the end of *after*. Clients: every worker process and the writer together, under load.", "");

  L.push("## Growth", "");
  const fl = (f, unit) => (f ? `${f.b.toExponential(3)} ${unit} per socket (intercept ${f2(f.a)}, r² ${f3(f.r2)})` : "— (fewer than two rungs ≥ 10)");
  L.push(`- **Memory, idle**: ${fl(analysis.fits.idle_rss, "MiB")}.`);
  L.push(`- **Memory, under load**: ${fl(analysis.fits.load_rss, "MiB")}.`);
  L.push(`- **CPU, idle**: ${fl(analysis.fits.idle_cpu, "CPU-s/s")}.`);
  L.push(`- **CPU, under load**: ${fl(analysis.fits.load_cpu, "CPU-s/s")}.`);
  L.push("", "Fitted over the rungs of 10 sockets and up (least squares, y = a + b·n). The exponent between neighbouring rungs, d log y / d log n — 1 is linear in the sockets, 0 flat:", "");
  L.push("| from → to | memory idle | memory load | CPU load | status → every p99 |", "| -- | --: | --: | --: | --: |");
  for (const e of analysis.exps) L.push(`| ${fint(e.from)} → ${fint(e.to)} | ${f2(e.idle_rss)} | ${f2(e.load_rss)} | ${f2(e.load_cpu)} | ${f2(e.every_p99)} |`);
  L.push("");
  L.push(`**Where it bends**: ${analysis.bends.length ? analysis.bends.map((b) => `at ${fint(b.at)} sockets, ${b.what}`).join("; ") : "nowhere on the rungs run — every step stays at or under linear (memory, CPU) and the latency to every watcher does not triple per ×10."}`, "");
  const floors = rungs.map((r) => r.floor_rss).filter((x) => x != null);
  L.push(`**The floor**: ${f1(baseline / MiB)} MiB before the ladder; after each rung ${rungs.map((r) => `${sup(r.rung)}: ${f1(r.floor_rss / MiB)}`).join(" · ")} MiB${floors.length > 1 ? ` — ${floors[floors.length - 1] - floors[0] > 0.1 * floors[0] ? "rising by more than 10%: look for a leak (or an allocator that keeps what it was given)" : "within 10% of the first rung's"}` : ""}.`, "");

  L.push("## Latency by write", "");
  L.push("| sockets | kind | writes | ack p50 / p99 | → every watcher p50 / p99 / max | per delivery p50 / p99 | reached every |", "| --: | -- | --: | --: | --: | --: | --: |");
  for (const r of rungs) for (const [k, v] of Object.entries(r.latency?.by_kind ?? {})) L.push(`| ${fint(r.n)} | ${k} | ${v.writes}${v.refused ? ` (${v.refused} refused)` : ""} | ${fms(v.ack_p50)} / ${fms(v.ack_p99)} | ${fms(v.every_p50)} / ${fms(v.every_p99)} / ${fms(v.every_max)} | ${fms(v.delivery_p50)} / ${fms(v.delivery_p99)} | ${v.reached_every}/${v.writes - v.refused} |`);
  L.push("", "A write is timed from just before its send to its arrival on each socket (one clock, `performance.timeOrigin + now()`, in every process); *every watcher* is the last socket's arrival. The clients' own event loops are inside these numbers — their CPU is in the table above.", "");
  L.push("## Reads under load", "");
  L.push("| sockets | reads/s offered | ok / sent | errors | " + run.plan.reads.map((x) => `${x.method} p50 / p99`).join(" | ") + " |");
  L.push("| --: | --: | --: | --: | " + run.plan.reads.map(() => "--:").join(" | ") + " |");
  for (const r of rungs) L.push(`| ${fint(r.n)} | ${f1(r.reads?.rate_per_s)} | ${fint(r.reads?.ok)} / ${fint(r.reads?.sent)} | ${r.reads?.errors ?? "—"} | ${run.plan.reads.map((x) => { const m = r.reads?.by_method?.[x.method]; return m ? `${fms(m.p50)} / ${fms(m.p99)}` : "—"; }).join(" | ")} |`);
  L.push("");
  L.push("## Per rung", "");
  for (const r of rungs) {
    L.push(`### ${fint(r.n)} socket${r.n > 1 ? "s" : ""}`, "");
    L.push(`- Load average ${r.loadavg_before.join(" / ")} → ${r.loadavg_after?.join(" / ")}; ${f1(r.mem_available_before / 1024 ** 3)} GiB available before.`);
    L.push(`- Opened ${fint(r.open.ok)}/${fint(r.n)} in ${f1(r.open.secs)} s over ${r.workers} worker(s), ${r.open.source_addresses} source address(es), ${r.open.keys} key(s); upgrade + 3 watches p50 ${fms(r.open.ms_p50)}, p99 ${fms(r.open.ms_p99)}.`);
    L.push(`- Server threads ≤ ${r.load?.threads_max ?? "—"}, descriptors ≤ ${fint(r.load?.fds_max)}.`);
    L.push(`- Events the watches heard: ${Object.entries(r.events_by_type ?? {}).map(([k, v]) => `${k} ${fint(v)}`).join(", ") || "—"}.`);
    if (r.problems?.length) L.push(`- **Problems**: ${r.problems.join("; ")}.`);
    L.push("");
  }
  L.push("## Where it stopped", "");
  L.push(stop ? `**${stop}**` : `Every rung asked for ran (${rungs.map((r) => sup(r.rung)).join(", ")}); none was refused by the box.`, "");
  L.push("## The curves", "");
  for (const f of Object.keys(charts)) L.push(`![${f}](${f})`, "");
  return L.join("\n") + "\n";
}

// ---------------------------------------------------------------------

async function main() {
  const o = parseArgs(process.argv.slice(2));
  const prod = path.join(os.homedir(), ".local", "share", "menger");
  const tmp = o.tmp ?? fs.mkdtempSync(path.join(os.tmpdir(), "menger-concurrency-"));
  if (path.resolve(tmp).startsWith(prod)) throw new Error(`--tmp must not be under ${prod}`);
  fs.mkdirSync(tmp, { recursive: true });
  fs.mkdirSync(path.join(o.out, "raw"), { recursive: true });
  if (o.build) {
    const b = spawnSync("cargo", ["build", "--release"], { cwd: REPO, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build --release failed");
  }
  if (!fs.existsSync(o.bin)) throw new Error(`missing ${o.bin} (cargo build --release, or --bin, or --build)`);

  const spec = machine(o);
  const top = 10 ** Math.max(...o.rungs);
  const caps = {
    max_connections: top + 1024,
    max_connections_per_ip: Math.min(top, o.socketsPerIp) + 64,
    max_connections_per_principal: Math.ceil(top / o.keys) + 64,
    idle_timeout_secs: 900,
    fds_needed: top * 2 + 4096,
  };
  console.log(`concurrency ladder: rungs ${o.rungs.map(sup).join(", ")} · ${spec.cpus} CPUs · ${f1(spec.mem_available_bytes / 1024 ** 3)} GiB available · load average ${spec.loadavg_start.join(" ")}`);
  console.log(`menger ${o.bin} · out ${o.out} · tmp ${tmp}`);

  // The operator (the server's --admin, and the writer) and the clients' keys.
  const operator = { keyFile: path.join(tmp, "operator.key") };
  operator.principal = makeKey(operator.keyFile);
  const keys = Array.from({ length: o.keys }, (_, k) => { const keyFile = path.join(tmp, `client-${k}.key`); return { keyFile, principal: makeKey(keyFile) }; });

  let srv = null, sampler = null;
  const ctx = { o, workers: [] };
  const cleanup = async () => {
    for (const w of ctx.workers ?? []) w.kill();
    sampler?.stop();
    if (srv?.alive()) { srv.child.kill("SIGTERM"); await Promise.race([srv.exited, sleep(5000)]); if (srv.alive()) srv.child.kill("SIGKILL"); }
    if (!o.keepTmp) fs.rmSync(tmp, { recursive: true, force: true });
  };
  process.on("SIGINT", () => cleanup().then(() => process.exit(130)));

  const rungs = [];
  let stop = null;
  let ok = true;
  try {
    srv = await startServer(o, tmp, caps, operator);
    console.log(`menger up on ws ${srv.ws}, shell ${srv.shell} (pid ${srv.pid}) in ${fms(srv.startup_ms)}; descriptors ${srv.fdNote}`);
    // Setup, as the operator: grant every key, find the writer's tickets.
    // Pool tickets start at In Review so each
    // write after is a real change.
    const op = await Rpc.open(srv.ws, srv.opToken);
    for (const k of keys) await op.must("members.grant", { principal: k.principal, scope: "*", role: "viewer" });
    const listed = await op.must("tickets.list", { summary: true, limit: 1000 });
    const work = listed.filter((x) => x.summary && !["scope", "doc"].includes(x.summary.kind)).map((x) => x.summary);
    const pool = work.slice(0, Math.min(100, work.length)).map((t) => ({ id: t.id, status: "In Review" }));
    if (pool.length < 2) throw new Error("the seeded store has too few tickets of work for the writer");
    for (const t of pool) await op.must("tickets.set_status", { id: t.id, status: "In Review" });
    const tokensFile = path.join(tmp, "tokens");
    const tokens = [];
    for (const [k, key] of keys.entries()) tokens.push((await mintSession(`ws://127.0.0.1:${srv.ws}`, key.keyFile, `perf-client-${k}`)).token);
    fs.writeFileSync(tokensFile, tokens.join("\n") + "\n", { mode: 0o600 });
    const plan = {
      reads: [
        { method: "tickets.health", params: {} },
        { method: "tickets.get", params: { id: pool[0].id } },
        { method: "tickets.list", params: { summary: true, limit: 50 } },
        { method: "tickets.nav.counts", params: { filter: "ready" } },
      ],
    };
    Object.assign(ctx, { srv, writer: op, pool, tokensFile, plan });
    sampler = new Sampler(srv, () => ctx.workers ?? [], o.sampleMs);
    ctx.sampler = sampler;
    sampler.start();
    await sleep(2000);
    const baseline = procSample(srv.pid).rss;

    let lastPerConn = null;
    for (const r of o.rungs) {
      const n = 10 ** r;
      // Will the box hold it? Memory by the last rung's cost per socket
      // (server and clients, ×2 for headroom), descriptors by the server's
      // limit and the source addresses' ephemeral ports.
      const avail = memAvailable();
      if (lastPerConn && avail != null) {
        const need = n * lastPerConn * 2;
        if (need > avail - 2 * 1024 ** 3) { stop = `stopped before ${fint(n)} sockets: by the last rung's ${f2(lastPerConn / MiB)} MiB per socket (server + clients) it wants ~${f1(need / 1024 ** 3)} GiB with headroom and ${f1(avail / 1024 ** 3)} GiB is available`; break; }
      }
      if (srv.fd_limit && n * 1.1 + 512 > srv.fd_limit.soft) { stop = `stopped before ${fint(n)} sockets: the server's descriptor limit is ${srv.fd_limit.soft}`; break; }
      const res = await runRung(r, ctx);
      rungs.push(res);
      fs.writeFileSync(path.join(o.out, "raw", `rung-${r}.json`), JSON.stringify(res, null, 2));
      if (res.idle && res.load) {
        const server = Math.max(0, res.load.rss_max - res.before.rss) / n;
        const clients = res.load.client_rss_max / n;
        lastPerConn = server + clients;
      }
      if (res.problems.length) {
        ok = false;
        stop = `stopped at ${fint(n)} sockets — the box did not hold it: ${res.problems.join("; ")}`;
        break;
      }
    }
    sampler.stop();
    op.close();

    const analysis = analyse(rungs, baseline);
    const charts = curves(o, rungs, sampler.samples);
    for (const [f, svg] of Object.entries(charts)) fs.writeFileSync(path.join(o.out, f), svg);
    const date = new Date().toISOString().slice(0, 10);
    const run = {
      date, spec, caps, options: o, rungs, stop, analysis, baseline, charts, poolSize: pool.length, plan,
      srvInfo: { startup_ms: srv.startup_ms, fdNote: srv.fdNote, fd_limit: srv.fd_limit }, loadavg_end: loadavg(),
      note: spec.loadavg_start[0] > spec.cpus * 0.1 ? `The machine was not quiet (other work running): read the CPU and latency numbers as an upper bound.` : "",
    };
    fs.writeFileSync(path.join(o.out, "raw", "run.json"), JSON.stringify({ ...run, charts: Object.keys(charts), samples: sampler.samples }, null, 2));
    const reportPath = path.join(o.out, `CONCURRENCY-${date}.md`);
    fs.writeFileSync(reportPath, report(o, run));
    console.log(`\nreport: ${reportPath}`);
    if (stop) console.log(stop);
  } finally {
    await cleanup();
  }

  if (o.smoke) {
    const want = o.rungs.length;
    if (!ok || rungs.length !== want) { console.error(`SMOKE FAILED: ${stop ?? `${rungs.length}/${want} rungs ran`}`); process.exit(1); }
    console.log(`SMOKE OK: rungs ${o.rungs.map(sup).join(", ")} — every socket opened, every write reached every watcher, every read answered`);
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
