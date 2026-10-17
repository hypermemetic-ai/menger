// CHAT-8: the chat probe — a chat of 10, 1,000, 10,000 and 100,000
// messages, or one message of 5,000 or 50,000 lines, opens as fast as a chat
// of ten, and the app holds it to its budgets.
//
// The magnitude harness's conventions (perf/magnitude.mjs, PERF-3): the
// release `menger` binary, ephemeral ports from the kernel, a temp database
// (never the user's), seeded through the server (`--seed-synthetic-chat`,
// CHAT-8: a scope CHATS-1, a ticket and a chat per size), budgets per row,
// a FAIL row for whatever exceeds its budget or breaks, and a report — the
// raw JSON beside it. The app is the release build given by --dist.
//
// What it measures, in headless Chromium at 1440×900 (the full-screen chat,
// CHAT-7, where the column is widest), per chat:
//
//   open      the ticket page live, then `?chat=ID&full`: open → the first
//             turns painted (a frame after they mount); the WebSocket bytes
//             received in between (the first page); at rest, the DOM's
//             nodes, the turns mounted, the JS heap and the wasm memory.
//   scroll    50 steps up, three viewports each (a fling): each older
//             page's request → the frame its turns are painted in; every
//             frame's time; long tasks (> 50 ms).
//   long      one message of N lines: open → its first lines painted; show
//             all → done, and the longest task while it renders.
//   writer    a writer posting 20 messages/s for 30 s into the open chat:
//             each post → painted; then, scrolled up, the turn being read
//             must not move while posts arrive.
//
//   node perf/chat-probe.mjs --dist /path/to/dist [--bin target/release/menger]
//        [--out perf/out/chat] [--runs 5] [--writer-secs 30] [--headed]
//
// Exit 0 when every row is within its budget, 1 when one is not, 2 on usage.

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";

import { makeKey, mintSession, signInContext } from "../scripts/throwaway-login.mjs";

const HERE = path.dirname(new URL(import.meta.url).pathname);
const REPO = path.resolve(HERE, "..");
const { chromium } = createRequire(new URL("./", import.meta.url))("playwright");

// ---------------------------------------------------------------------
// Options

function parse(argv) {
  const o = {
    bin: process.env.CARGO_TARGET_DIR ? path.join(process.env.CARGO_TARGET_DIR, "release", "menger") : path.join(REPO, "target", "release", "menger"),
    dist: null,
    out: path.join(REPO, "perf", "out", "chat"),
    tmp: os.tmpdir(),
    runs: 5,
    writerSecs: 30,
    headed: false,
    sizes: [10, 1000, 10000, 100000],
    lines: [5000, 50000],
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i], next = () => argv[++i] ?? usage(`${a} needs a value`);
    if (a === "--bin") o.bin = path.resolve(next());
    else if (a === "--dist") o.dist = path.resolve(next());
    else if (a === "--out") o.out = path.resolve(next());
    else if (a === "--tmp") o.tmp = path.resolve(next());
    else if (a === "--runs") o.runs = Number(next());
    else if (a === "--writer-secs") o.writerSecs = Number(next());
    else if (a === "--sizes") o.sizes = next().split(",").map(Number);
    else if (a === "--lines") o.lines = next().split(",").map(Number);
    else if (a === "--headed") o.headed = true;
    else if (a === "-h" || a === "--help") usage();
    else usage(`unknown option ${a}`);
  }
  if (!o.dist) usage("--dist DIR: the app's release build (trunk build --release --dist DIR)");
  if (!fs.existsSync(o.bin)) usage(`no menger binary at ${o.bin} (cargo build --release -p menger)`);
  return o;
}

function usage(msg) {
  if (msg) console.error(`chat-probe: ${msg}`);
  console.error("usage: node perf/chat-probe.mjs --dist DIR [--bin PATH] [--out DIR] [--runs N] [--writer-secs S] [--sizes 10,1000] [--lines 5000] [--headed]");
  process.exit(2);
}

// ---------------------------------------------------------------------
// Budgets (CHAT-8's table)

const BUDGET = {
  paintMs: 300, // open → first messages painted, at every size
  flatPct: 20, // mounted turns and DOM nodes within ±20% of the 1,000 case
  pageP95Ms: 150, // an older page's fetch → painted, p95
  scrollLongTaskMs: 100, // no long task over this while scrolling
  longFirstMs: 300, // a long message: open → its first lines painted
  longTaskMs: 100, // show all: no task over this
  writerP95Ms: 250, // a post → painted, p95
  moveTolerancePx: 1, // scrolled up, the turn read does not move
};

// ---------------------------------------------------------------------
// Helpers

const now = () => performance.now();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const median = (v) => {
  const s = [...v].sort((a, b) => a - b);
  if (!s.length) return null;
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const pct = (v, p) => {
  const s = [...v].sort((a, b) => a - b);
  return s.length ? s[Math.min(s.length - 1, Math.ceil((p / 100) * s.length) - 1)] : null;
};
const fmtMs = (ms) => (ms == null ? "—" : ms < 10 ? `${ms.toFixed(1)} ms` : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`);
const fmtBytes = (b) => (b == null ? "—" : b < 1024 ? `${b} B` : b < 1024 ** 2 ? `${(b / 1024).toFixed(1)} KiB` : `${(b / 1024 ** 2).toFixed(1)} MiB`);
const fmtInt = (n) => (n == null ? "—" : Math.round(n).toLocaleString("en-US"));

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
    s.once("connect", () => (s.destroy(), resolve(true)));
    s.once("error", () => resolve(false));
  });
}

// ---------------------------------------------------------------------
// The server

async function startServer(o, spec) {
  const work = fs.mkdtempSync(path.join(o.tmp, "menger-chat-probe-"));
  const db = path.join(work, "probe.db");
  // ACCESS-24/52: login always — the probe's own key, the server's --admin
  // and the chats' reader (the seed makes the first --admin its reader).
  const keyFile = path.join(work, "probe.key");
  const principal = makeKey(keyFile);
  const [ws, shell] = [await freePort(), await freePort()];
  const env = { ...process.env, PLEXUS_NO_REGISTRY: "1" };
  for (const k of ["MENGER_DB_PATH", "MENGER_APP_DIST", "RUST_LOG", "MENGER_SEED_SYNTHETIC", "MENGER_SEED_SYNTHETIC_CHAT"]) delete env[k];
  const args = ["--port", ws, "--shell-port", shell, "--no-rest", "--db-path", db, "--app-dist", o.dist, "--auth-provider", "key", "--admin", principal, "--seed-synthetic-chat", spec].map(String);
  const child = spawn(o.bin, args, { env, stdio: ["ignore", "ignore", "pipe"] });
  let stderr = "";
  child.stderr.on("data", (d) => (stderr += d.toString()));
  const t0 = now();
  while (now() - t0 < 600_000) {
    if (child.exitCode !== null) throw new Error(`server exited: ${stderr.trim().split("\n").pop()}`);
    if ((await tcpUp(ws)) && (await tcpUp(shell))) break;
    await sleep(200);
  }
  // Which chat is on which ticket, as the seed said.
  const chats = [...stderr.matchAll(/seeded chat (\d+) on (CHATS-\d+): (\d+) message/g)].map((m) => ({ id: +m[1], ticket: m[2], messages: +m[3] }));
  const seedMs = +(stderr.match(/seeded \d+ synthetic chat\(s\) in ([\d.]+) ms/)?.[1] ?? NaN);
  const { token } = await mintSession(`ws://127.0.0.1:${ws}`, keyFile, "probe");
  return { child, ws, shell, db, chats, seedMs, token, stop: async () => {
    child.kill("SIGKILL");
    await sleep(300);
    fs.rmSync(work, { recursive: true, force: true });
  } };
}

// ---------------------------------------------------------------------
// In the page: timing hooks, installed before the app starts.

const HOOKS = () => {
  window.__probe = { sends: [], recvs: [], bytes: 0, longtasks: [], frames: [] };
  addEventListener("TrunkApplicationStarted", (e) => (window.__probe.wasm = e.detail.wasm));
  const Native = window.WebSocket;
  window.WebSocket = class extends Native {
    constructor(...a) {
      super(...a);
      this.addEventListener("message", (e) => {
        const p = window.__probe;
        const n = typeof e.data === "string" ? e.data.length : e.data.byteLength ?? 0;
        p.bytes += n;
        if (typeof e.data === "string" && e.data.includes('"type":"page"')) p.recvs.push(performance.now());
      });
    }
    send(d) {
      if (typeof d === "string" && d.includes("chats.messages")) window.__probe.sends.push({ at: performance.now(), before: d.includes('"before"') });
      return super.send(d);
    }
  };
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) window.__probe.longtasks.push({ at: e.startTime, ms: e.duration });
    }).observe({ type: "longtask", buffered: true });
  } catch {}
};

async function page(browser, o, base, token) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await signInContext(ctx, base.replace(/\/$/, ""), token);
  await ctx.addInitScript(HOOKS);
  const p = await ctx.newPage();
  const errors = [];
  p.on("pageerror", (e) => errors.push(e.message));
  await p.goto(base);
  await p.waitForFunction(() => document.querySelector("#conn")?.className.includes("live"), null, { timeout: 30_000 });
  return { ctx, p, errors };
}

// ---------------------------------------------------------------------
// The measurements

async function openChat(p, ticket, chat) {
  // Start on the ticket page, live; then open the chat full screen.
  await p.evaluate((t) => (location.hash = `#/ticket/${t}`), ticket);
  await p.waitForSelector("#t-chats", { timeout: 30_000 });
  await p.waitForTimeout(400);
  return p.evaluate(
    async ({ ticket, chat, painted }) => {
      window.__probe.bytes = 0;
      const t0 = performance.now();
      location.hash = `#/ticket/${ticket}?chat=${chat}&full`;
      const at = await new Promise((resolve) => {
        const pred = eval(painted);
        const tick = () => (pred() ? requestAnimationFrame(() => resolve(performance.now())) : requestAnimationFrame(tick));
        tick();
      });
      return { ms: at - t0, bytes: window.__probe.bytes };
    },
    { ticket, chat, painted: `() => document.querySelector('#chatfull .trow')` },
  );
}

async function atRest(p) {
  await p.waitForTimeout(1200);
  return p.evaluate(() => ({
    dom: document.getElementsByTagName("*").length,
    mounted: document.querySelectorAll("#chatfull .trow").length,
    heap: performance.memory?.usedJSHeapSize ?? null,
    wasm: window.__probe.wasm?.memory?.buffer?.byteLength ?? null,
    held: +(document.querySelector("#chatfull .tlist")?.dataset.held ?? 0),
    page: Math.min(200, Math.max(20, Math.ceil(innerHeight / 24) * 2)),
  }));
}

async function scrollUp(p, steps) {
  return p.evaluate(async (steps) => {
    const el = document.querySelector("#chatfull .pmsgs");
    const list = document.querySelector("#chatfull .tlist");
    const probe = window.__probe;
    probe.longtasks = [];
    const t0 = performance.now();
    const frames = [];
    let last = performance.now();
    let running = true;
    const frame = (t) => {
      frames.push(t - last);
      last = t;
      if (running) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
    // Each time the window's first held message moves back: the frame it is painted in.
    const grew = [];
    let first = +list.dataset.first;
    const obs = new MutationObserver(() => {
      const f = +list.dataset.first;
      if (f && f < first) requestAnimationFrame((t) => grew.push(t));
      first = f;
    });
    obs.observe(list, { attributes: true, attributeFilter: ["data-first"] });
    const sentFrom = probe.sends.length;
    for (let i = 0; i < steps; i++) {
      el.scrollTop = Math.max(0, el.scrollTop - 3 * el.clientHeight);
      await new Promise((r) => setTimeout(r, 120));
    }
    await new Promise((r) => setTimeout(r, 1500));
    running = false;
    obs.disconnect();
    // Each older page's request, paired with the next paint of a grown window.
    const asks = probe.sends.slice(sentFrom).filter((s) => s.before).map((s) => s.at);
    const pages = asks.map((at) => {
      const done = grew.find((g) => g >= at);
      return done == null ? null : done - at;
    });
    return {
      pages,
      requests: asks.length,
      frames: frames.slice(1),
      longtasks: probe.longtasks.filter((l) => l.at >= t0).map((l) => l.ms),
      ms: performance.now() - t0,
    };
  }, steps);
}

async function longMessage(p, ticket, chat) {
  const open = await openChat(p, ticket, chat);
  await p.waitForSelector("#chatfull .tmore", { timeout: 10_000 });
  const shown = await p.evaluate(async () => {
    window.__probe.longtasks = [];
    const t0 = performance.now();
    document.querySelector("#chatfull .tmore").click();
    const at = await new Promise((resolve) => {
      const deadline = performance.now() + 120_000;
      const tick = () => (!document.querySelector("#chatfull .tmore") || performance.now() > deadline ? requestAnimationFrame(() => resolve(performance.now())) : requestAnimationFrame(tick));
      tick();
    });
    await new Promise((r) => setTimeout(r, 300));
    const tasks = window.__probe.longtasks.filter((l) => l.at >= t0).map((l) => l.ms);
    return { ms: at - t0, longest: tasks.length ? Math.max(...tasks) : 0, longtasks: tasks.length, done: !document.querySelector("#chatfull .tmore") };
  });
  return { open, shown };
}

/// A writer over its own socket: `chats.post`, one JSON-RPC call each.
async function writer(port, token, chat, perSec, secs, onSent) {
  const ws = new WebSocket(`ws://127.0.0.1:${port}`, { headers: { Authorization: `Bearer ${token}` } });
  await new Promise((res, rej) => ((ws.onopen = res), (ws.onerror = rej)));
  ws.onmessage = () => {};
  const total = perSec * secs;
  for (let k = 0; k < total; k++) {
    const token = `probe-${process.pid}-${k}`;
    onSent(token, Date.now());
    ws.send(JSON.stringify({ jsonrpc: "2.0", id: k + 1, method: "menger.call", params: { method: "chats.post", params: { chat, body: `A reading from the writer, ${token}.` } } }));
    await sleep(1000 / perSec);
  }
  await sleep(1500);
  ws.close();
}

async function writerRow(p, o, port, token, chat) {
  // Watch for each token's turn and stamp the frame it is painted in.
  await p.evaluate(() => {
    window.__painted = {};
    const seen = new Set();
    new MutationObserver(() => {
      for (const el of document.querySelectorAll("#chatfull .trow")) {
        const m = el.textContent.match(/probe-\d+-\d+/);
        if (m && !seen.has(m[0])) {
          seen.add(m[0]);
          const t = m[0];
          requestAnimationFrame(() => (window.__painted[t] = Date.now()));
        }
      }
    }).observe(document.querySelector("#chatfull .tlist"), { childList: true, subtree: true });
  });
  const sent = {};
  await writer(port, token, chat, 20, o.writerSecs, (t, at) => (sent[t] = at));
  const painted = await p.evaluate(() => window.__painted);
  const lat = Object.entries(sent).map(([t, at]) => (painted[t] ? painted[t] - at : null));
  const missing = lat.filter((x) => x == null).length;
  // Scrolled up: what is read stays put while posts arrive.
  const moved = await p.evaluate(async () => {
    const el = document.querySelector("#chatfull .pmsgs");
    el.scrollTop = Math.max(0, el.scrollTop - 2 * el.clientHeight);
    await new Promise((r) => setTimeout(r, 600));
    const row = [...document.querySelectorAll("#chatfull .trow")].find((r) => r.getBoundingClientRect().bottom > el.getBoundingClientRect().top + 10);
    window.__watch = { mid: row?.dataset.mid, top: row?.getBoundingClientRect().top };
    return window.__watch.mid;
  });
  await writer(port, token, chat, 20, 3, () => {});
  const after = await p.evaluate(() => {
    const row = document.querySelector(`#chatfull .trow[data-mid="${window.__watch.mid}"]`);
    return row ? row.getBoundingClientRect().top - window.__watch.top : null;
  });
  return { latencies: lat.filter((x) => x != null), missing, moved: after, anchor: moved };
}

// ---------------------------------------------------------------------
// The run

async function main() {
  const o = parse(process.argv.slice(2));
  fs.mkdirSync(o.out, { recursive: true });
  const spec = [...o.sizes.map(String), ...o.lines.map((n) => `L${n}`)].join(",");
  console.log(`chat-probe: seeding ${spec} …`);
  const server = await startServer(o, spec);
  console.log(`chat-probe: seeded in ${fmtMs(server.seedMs)}; ws :${server.ws}, app :${server.shell}`);
  const browser = await chromium.launch({ headless: !o.headed });
  const base = `http://127.0.0.1:${server.shell}/`;
  const rows = [];
  const fail = (row, why) => (row.fail = [...(row.fail ?? []), why]);
  try {
    // open + at rest + scroll, per size
    for (const c of server.chats.filter((c) => c.messages > 1)) {
      const row = { kind: "size", messages: c.messages, chat: c.id, ticket: c.ticket, opens: [], bytes: [], rest: null, scroll: null };
      for (let r = 0; r < o.runs; r++) {
        const { ctx, p, errors } = await page(browser, o, base, server.token);
        try {
          const open = await openChat(p, c.ticket, c.id);
          row.opens.push(open.ms);
          row.bytes.push(open.bytes);
          if (r === 0) {
            row.rest = await atRest(p);
            row.scroll = await scrollUp(p, 50);
          }
          if (errors.length) fail(row, `page error: ${errors[0]}`);
        } catch (e) {
          fail(row, String(e.message).split("\n")[0]);
        } finally {
          await ctx.close();
        }
      }
      row.paint = median(row.opens);
      row.firstBytes = median(row.bytes);
      if (row.scroll) {
        const pages = row.scroll.pages.filter((x) => x != null);
        row.pageP95 = pct(pages, 95);
        row.pages = pages.length;
        row.pageTimeouts = row.scroll.pages.length - pages.length;
        row.longest = row.scroll.longtasks.length ? Math.max(...row.scroll.longtasks) : 0;
        row.longtasks = row.scroll.longtasks.length;
        row.frameP95 = pct(row.scroll.frames, 95);
      }
      console.log(`  ${fmtInt(c.messages)} messages: painted ${fmtMs(row.paint)}, ${fmtBytes(row.firstBytes)}, ${row.rest?.mounted} mounted, ${row.rest?.dom} nodes; pages p95 ${fmtMs(row.pageP95)} (${row.pages}), long tasks ${row.longtasks} (max ${fmtMs(row.longest)})`);
      rows.push(row);
    }
    // the long messages
    for (const c of server.chats.filter((c) => c.messages === 1)) {
      const lines = o.lines[server.chats.filter((x) => x.messages === 1).indexOf(c)];
      const row = { kind: "long", lines, chat: c.id, ticket: c.ticket };
      const { ctx, p, errors } = await page(browser, o, base, server.token);
      try {
        const r = await longMessage(p, c.ticket, c.id);
        Object.assign(row, { first: r.open.ms, firstBytes: r.open.bytes, showAll: r.shown.ms, longest: r.shown.longest, longtasks: r.shown.longtasks, done: r.shown.done });
        if (errors.length) fail(row, `page error: ${errors[0]}`);
      } catch (e) {
        fail(row, String(e.message).split("\n")[0]);
      } finally {
        await ctx.close();
      }
      console.log(`  one message of ${fmtInt(lines)} lines: first lines ${fmtMs(row.first)}; show all ${fmtMs(row.showAll)}, longest task ${fmtMs(row.longest)}`);
      rows.push(row);
    }
    // the writer, into the 1,000-message chat
    {
      const c = server.chats.find((x) => x.messages === 1000) ?? server.chats[0];
      const row = { kind: "writer", chat: c.id, ticket: c.ticket, perSec: 20, secs: o.writerSecs };
      const { ctx, p, errors } = await page(browser, o, base, server.token);
      try {
        await openChat(p, c.ticket, c.id);
        await p.waitForTimeout(800);
        const w = await writerRow(p, o, server.ws, server.token, c.id);
        Object.assign(row, { p50: median(w.latencies), p95: pct(w.latencies, 95), posts: w.latencies.length, missing: w.missing, moved: w.moved });
        if (errors.length) fail(row, `page error: ${errors[0]}`);
      } catch (e) {
        fail(row, String(e.message).split("\n")[0]);
      } finally {
        await ctx.close();
      }
      console.log(`  writer 20/s × ${o.writerSecs} s: p50 ${fmtMs(row.p50)}, p95 ${fmtMs(row.p95)}, ${row.missing ?? "?"} not painted; scrolled up, moved ${row.moved} px`);
      rows.push(row);
    }
  } finally {
    await browser.close();
    await server.stop();
  }
  // Budgets
  const ref = rows.find((r) => r.kind === "size" && r.messages === 1000);
  for (const r of rows) {
    if (r.kind === "size") {
      if (!(r.paint <= BUDGET.paintMs)) fail(r, `painted ${fmtMs(r.paint)} > ${BUDGET.paintMs} ms`);
      if (ref?.rest && r.rest) {
        // A chat smaller than one page is mounted whole: its bound is all of it.
        const under = r.messages < ref.rest.page;
        for (const k of under ? ["dom"] : ["mounted", "dom"]) {
          const d = (100 * Math.abs(r.rest[k] - ref.rest[k])) / ref.rest[k];
          if (d > BUDGET.flatPct) fail(r, `${k} ${r.rest[k]} is ${d.toFixed(0)}% off the 1,000 case (${ref.rest[k]})`);
        }
      }
      if (r.pages && !(r.pageP95 <= BUDGET.pageP95Ms)) fail(r, `page p95 ${fmtMs(r.pageP95)} > ${BUDGET.pageP95Ms} ms`);
      if (r.pageTimeouts) fail(r, `${r.pageTimeouts} page(s) never painted`);
      if (r.longest > BUDGET.scrollLongTaskMs) fail(r, `a long task of ${fmtMs(r.longest)} while scrolling`);
    } else if (r.kind === "long") {
      if (!(r.first <= BUDGET.longFirstMs)) fail(r, `first lines ${fmtMs(r.first)} > ${BUDGET.longFirstMs} ms`);
      if (!(r.longest <= BUDGET.longTaskMs)) fail(r, `a task of ${fmtMs(r.longest)} while showing all`);
      if (!r.done) fail(r, "show all never finished");
    } else if (r.kind === "writer") {
      if (!(r.p95 <= BUDGET.writerP95Ms)) fail(r, `post → painted p95 ${fmtMs(r.p95)} > ${BUDGET.writerP95Ms} ms`);
      if (r.missing) fail(r, `${r.missing} post(s) never painted`);
      if (!(Math.abs(r.moved ?? 99) <= BUDGET.moveTolerancePx)) fail(r, `scrolled up, the view moved ${r.moved} px`);
    }
  }
  const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, "-");
  const raw = { when: new Date().toISOString(), machine: { cpu: os.cpus()[0]?.model, cores: os.cpus().length, mem: os.totalmem(), kernel: os.release(), node: process.version }, seedMs: server.seedMs, budget: BUDGET, rows };
  fs.writeFileSync(path.join(o.out, `chat-probe-${stamp}.json`), JSON.stringify(raw, null, 2));
  const md = report(raw);
  fs.writeFileSync(path.join(o.out, `chat-probe-${stamp}.md`), md);
  console.log(`\n${md}\nchat-probe: report ${path.join(o.out, `chat-probe-${stamp}.md`)}`);
  process.exit(rows.some((r) => r.fail?.length) ? 1 : 0);
}

function report(raw) {
  const L = [];
  const v = (r) => (r.fail?.length ? `**FAIL** (${r.fail.join("; ").replace(/\|/g, "/")})` : "ok");
  L.push(`### Chat probe — ${raw.when.slice(0, 16)}Z`);
  L.push("");
  L.push(`${raw.machine.cpu}, ${raw.machine.cores} cores, ${fmtBytes(raw.machine.mem)}, Linux ${raw.machine.kernel}, Node ${raw.machine.node}; headless Chromium at 1440×900, the full-screen chat; the release server on kernel ports over a temp database (login by its own key, ACCESS-52), seeded with \`--seed-synthetic-chat\` in ${fmtMs(raw.seedMs)}.`);
  L.push("");
  L.push("| chat | painted (median) | first page | turns mounted · DOM nodes at rest | JS heap · wasm | older page p95 (n) | frame p95 · long tasks scrolling | verdict |");
  L.push("| -- | -- | -- | -- | -- | -- | -- | -- |");
  for (const r of raw.rows.filter((r) => r.kind === "size")) {
    L.push(`| ${fmtInt(r.messages)} messages | ${fmtMs(r.paint)} | ${fmtBytes(r.firstBytes)} | ${r.rest?.mounted ?? "—"}${r.rest && r.messages < r.rest.page ? " (all: under a page)" : ""} · ${fmtInt(r.rest?.dom)} | ${fmtBytes(r.rest?.heap)} · ${fmtBytes(r.rest?.wasm)} | ${fmtMs(r.pageP95)} (${r.pages ?? 0}) | ${fmtMs(r.frameP95)} · ${r.longtasks ?? "—"}, max ${fmtMs(r.longest)} | ${v(r)} |`);
  }
  L.push("");
  L.push("| one message | first lines | show all → done | longest task | verdict |");
  L.push("| -- | -- | -- | -- | -- |");
  for (const r of raw.rows.filter((r) => r.kind === "long")) L.push(`| ${fmtInt(r.lines)} lines | ${fmtMs(r.first)} | ${fmtMs(r.showAll)} | ${fmtMs(r.longest)} (${r.longtasks ?? 0} over 50 ms) | ${v(r)} |`);
  L.push("");
  L.push("| writer | posts painted | p50 | p95 | scrolled up, moved | verdict |");
  L.push("| -- | -- | -- | -- | -- | -- |");
  for (const r of raw.rows.filter((r) => r.kind === "writer")) L.push(`| ${r.perSec}/s × ${r.secs} s | ${r.posts ?? "—"} (${r.missing ?? "—"} not) | ${fmtMs(r.p50)} | ${fmtMs(r.p95)} | ${r.moved ?? "—"} px | ${v(r)} |`);
  L.push("");
  L.push(`Budgets: painted ≤ ${BUDGET.paintMs} ms at every size; mounted and DOM within ±${BUDGET.flatPct}% of the 1,000 case; an older page ≤ ${BUDGET.pageP95Ms} ms p95; no long task over ${BUDGET.scrollLongTaskMs} ms scrolling; a long message's first lines ≤ ${BUDGET.longFirstMs} ms and no task over ${BUDGET.longTaskMs} ms showing all; post → painted ≤ ${BUDGET.writerP95Ms} ms p95, and nothing read moves.`);
  return L.join("\n");
}

main().catch((e) => {
  console.error(`chat-probe: ${e.stack ?? e}`);
  process.exit(1);
});
