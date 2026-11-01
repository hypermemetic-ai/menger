#!/usr/bin/env node
// V2-20: the ringer — questions and the queue under load, on a release
// binary, at 10³, 10⁵ and 10⁶ tickets holding 10⁴ open inputs and 10⁴ queued
// (a ticket is queued once, so 10³ tickets hold 10³ entries).
//
//   node perf/ringer.mjs [--rungs 3,5,6] [--inputs 10000] [--queued 10000] [--samples 500] [--page] [--no-build] [--keep]
//
// One throwaway at each rung (scripts/ui-throwaway.mjs: kernel-chosen ports,
// its files in a scratch directory), filled through the wire, then:
//
//   reads     `queued`, `inputs list --state open`, `list` under the words
//             `asking`, `queued`, `queued now`, `asked` — one socket, one at
//             a time, p50 and p99
//   writes    ask, answer, queue, reorder (a raise) — the same
//   burst     a read while 32 writers write flat out
//   fan-out   a write with 100 watches open, its send to its arrival on the
//             last; 100 plain, then 100 asking a word of the queue
//   parallel  1,000 asks in flight over one socket, and over 100
//   held      200 `await_reply` held: a write beside them, and an answer
//             to each one's arrival
//   restart   SIGKILL, then a start with all of it: time to its first
//             signed-in answer
//   memory    the server's RSS before the fill and after, per input and
//             entry
//   page      with --page: #/queue's first row, cold, and a queued row's
//             frame to its row, in Chromium
//
// Budgets: a read 5 ms p99, a write 20 ms p99, a page 0.3 s (the board's).
// Prints markdown rows for perf/notes.md and a JSON line per rung. The ports
// are the kernel's, never prod's or test's; the work directory is the
// caller's to remove (scripts/cleanup-gate.sh).
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { connect } from "../deploy/menger-call.mjs";
import { makeKey, readToken, sessionCookies, writeSession } from "../scripts/throwaway-login.mjs";
import { INIT, TARGET, Throwaway, build, bundle, playwright, push, sleep, workDir } from "../scripts/ui-throwaway.mjs";

const args = process.argv.slice(2);
const flag = (n) => args.includes(n);
const opt = (n, d) => (args.includes(n) ? args[args.indexOf(n) + 1] : d);
const RUNGS = opt("--rungs", "3,5,6").split(",").map(Number);
const INPUTS = Number(opt("--inputs", "10000"));
const QUEUED = Number(opt("--queued", "10000"));
const SAMPLES = Number(opt("--samples", "500"));
const BURST_MS = 5000;
const READ_BUDGET = 5, WRITE_BUDGET = 20, PAGE_BUDGET = 300;

const pct = (xs, p) => {
  const s = [...xs].sort((a, b) => a - b);
  return s.length ? s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))] : NaN;
};
const ms = (x) => (Number.isNaN(x) ? "–" : x < 10 ? x.toFixed(2) : x.toFixed(1));

/** One signed-in socket, many calls at once. */
class Client {
  static async open(port, token) {
    const ws = await connect(`ws://127.0.0.1:${port}`, { Authorization: `Bearer ${token}` });
    return new Client(ws);
  }
  constructor(ws) {
    this.ws = ws;
    this.next = 1;
    this.byId = new Map();
    this.bySub = new Map();
    ws.onmessage = (text) => this.heard(text);
    ws.onclose = (why) => {
      this.closed = why;
      for (const c of [...this.byId.values(), ...this.bySub.values()]) c.end?.(new Error(String(why)));
    };
  }
  heard(text) {
    let m;
    try {
      m = JSON.parse(text);
    } catch {
      return;
    }
    if (m.id != null && this.byId.has(m.id)) {
      const c = this.byId.get(m.id);
      this.byId.delete(m.id);
      if (m.error) return c.end(new Error(m.error.message ?? JSON.stringify(m.error)));
      c.sub = m.result;
      this.bySub.set(m.result, c);
      for (const held of c.early ?? []) c.item(held);
      return;
    }
    const sub = m.params?.subscription;
    if (sub === undefined) return;
    const c = this.bySub.get(sub);
    if (!c) return;
    const item = m.params.result;
    if (item?.type === "done") return c.end();
    c.item(item);
    if (item?.type === "error") c.end();
  }
  /** A stream: `onItem` each data item; `.done` a promise; `.close()` lets it go. */
  stream(method, params, onItem) {
    const id = this.next++;
    let end;
    const done = new Promise((resolve, reject) => (end = (e) => (e ? reject(e) : resolve())));
    const c = {
      item: (i) => {
        if (i?.type === "data") onItem(i.content);
        else if (i?.type === "error") c.failed = i;
      },
      end: (e) => {
        if (c.sub) this.bySub.delete(c.sub);
        end(e);
      },
    };
    this.byId.set(id, c);
    this.ws.send(JSON.stringify({ jsonrpc: "2.0", id, method: "menger.call", params: { method, params } }));
    return { done, close: () => c.end(), failed: () => c.failed };
  }
  /** A finite call: its events, `rows` read out. */
  async call(method, params = {}) {
    const out = [];
    const s = this.stream(method, params, (e) => (e?.type === "rows" ? out.push(...e.items) : out.push(e)));
    await s.done;
    if (s.failed()) throw new Error(`${method}: ${JSON.stringify(s.failed()).slice(0, 300)}`);
    return out;
  }
  close() {
    this.ws.close();
  }
}

async function timed(n, f) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const t = performance.now();
    await f(i);
    out.push(performance.now() - t);
  }
  return out;
}

/** `f(i)` for i in 0..n, `width` at a time. */
async function pool(n, width, f) {
  let i = 0;
  await Promise.all(Array.from({ length: width }, async () => {
    while (i < n) await f(i++);
  }));
}

const rssKb = (pid) => Number(/VmRSS:\s+(\d+)/.exec(fs.readFileSync(`/proc/${pid}/status`, "utf8"))?.[1] ?? 0);

function row(sheet, what, xs, budget) {
  const [p50, p99, max] = [pct(xs, 50), pct(xs, 99), pct(xs, 100)];
  const over = budget != null && p99 > budget;
  sheet.push({ what, n: xs.length, p50, p99, max, budget, over });
  console.error(`  ${what.padEnd(46)} p50 ${ms(p50).padStart(7)}  p99 ${ms(p99).padStart(7)}  max ${ms(max).padStart(7)}${over ? "  OVER" : ""}`);
}

async function rung(exp, built) {
  const N = 10 ** exp;
  const work = workDir(`ringer-${exp}`);
  const sheet = [];
  const facts = {};
  console.error(`\n== 10^${exp} tickets — ${work}`);
  const keyFile = path.join(work, "owner.key");
  const principal = makeKey(keyFile);
  const signerFile = path.join(work, "signer.key");
  const signer = makeKey(signerFile).replace(/^key:/, "");
  const env = { MENGER_MAX_CALLS_PER_CONNECTION: "4096", MENGER_MAX_CONNECTIONS_PER_IP: "4096", MENGER_MAX_CONNECTIONS_PER_PRINCIPAL: "4096" };
  Object.assign(process.env, env);
  const server = new Throwaway(work, "release");
  let t0 = Date.now();
  await server.start(principal, signer, N);
  console.error(`  up, ${N} seeded, in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  facts.seeded_s = (Date.now() - t0) / 1000;
  const sessionFile = path.join(work, "session");
  await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, sessionFile, "owner");
  const token = readToken(sessionFile);
  const main = await Client.open(server.ws, token);
  const rss0 = rssKb(server.child.pid);

  // The tickets: every id a page gives, skipping the scopes' roots.
  let ids = [];
  for (let after = null; ids.length < Math.min(N, QUEUED + 3000); ) {
    const page = await main.call("tickets.list", { summary: true, limit: 5000, fields: ["id"], ...(after ? { after } : {}) });
    ids.push(...page.filter((e) => e.type === "summary").map((e) => e.summary.id));
    after = page.find((e) => e.type === "page")?.next;
    if (!after) break;
  }
  console.error(`  ${ids.length} ids`);

  // The fill: 10⁴ open questions, round the tickets; then the queue.
  t0 = Date.now();
  const asked = [];
  await pool(INPUTS, 16, async (i) => {
    const ticket = ids[i % ids.length];
    const e = await main.call("tickets.inputs.ask", { ticket, question: `Question ${i} of the ringer?` });
    const id = e.find((x) => x.type === "input_changed")?.input?.id;
    asked[i] = { ticket, id };
  });
  facts.fill_inputs_s = (Date.now() - t0) / 1000;
  console.error(`  ${INPUTS} asked in ${facts.fill_inputs_s.toFixed(1)} s`);
  t0 = Date.now();
  const entries = Math.min(QUEUED, ids.length);
  const U = ["whenever", "today", "now", "blocking"];
  const inputOf = new Map();
  for (const a of asked) if (!inputOf.has(a.ticket)) inputOf.set(a.ticket, a.id);
  await pool(entries, 16, async (i) => {
    const id = ids[i];
    const urgency = U[i % 3];
    await main.call("tickets.queue", { id, urgency, reason: `the ringer's ${i}`, ...(inputOf.has(id) && i % 2 ? { input: inputOf.get(id) } : {}) });
  });
  facts.fill_queue_s = (Date.now() - t0) / 1000;
  facts.entries = entries;
  console.error(`  ${entries} queued in ${facts.fill_queue_s.toFixed(1)} s`);
  const rss1 = rssKb(server.child.pid);
  facts.rss_kb = [rss0, rss1];
  facts.bytes_per_item = ((rss1 - rss0) * 1024) / (INPUTS + entries);

  // The reads.
  console.error("  reads");
  const reader = await Client.open(server.ws, token);
  const reads = [
    ["queued, a page of 200", () => reader.call("tickets.queued", { limit: 200 })],
    ["queued, a page of 50", () => reader.call("tickets.queued", { limit: 50 })],
    ["inputs list open, 200", () => reader.call("tickets.inputs.list", { state: "open", limit: 200 })],
    ["inputs list open, 50", () => reader.call("tickets.inputs.list", { state: "open", limit: 50 })],
    ["list `asking`, 200 summaries", () => reader.call("tickets.list", { filter: "asking", summary: true, limit: 200 })],
    ["list `queued`, 200 summaries", () => reader.call("tickets.list", { filter: "queued", summary: true, limit: 200 })],
    ["list `queued now`, 200 summaries", () => reader.call("tickets.list", { filter: "queued now", summary: true, limit: 200 })],
    ["list `asked`, 200 summaries", () => reader.call("tickets.list", { filter: "asked", summary: true, limit: 200 })],
    ["nav counts `asking`", () => reader.call("tickets.nav.counts", { filter: "asking" })],
  ];
  for (const [what, f] of reads) row(sheet, what, await timed(SAMPLES, f), READ_BUDGET);

  // The writes.
  console.error("  writes");
  const spare = ids.slice(0, Math.max(1, Math.min(ids.length, 2000)));
  let w = 0;
  const pick = () => spare[w++ % spare.length];
  const open = asked.slice();
  row(sheet, "ask", await timed(SAMPLES, () => main.call("tickets.inputs.ask", { ticket: pick(), question: "One more?" })), WRITE_BUDGET);
  row(sheet, "answer", await timed(SAMPLES, (i) => main.call("tickets.inputs.answer", { ticket: open[i].ticket, input: open[i].id, value: { type: "text", text: "yes" } })), WRITE_BUDGET);
  const fresh = ids.slice(entries);
  row(sheet, "queue, a new entry", await timed(Math.min(SAMPLES, Math.max(fresh.length, 1)), (i) => {
    const id = fresh[i] ?? ids[i];
    return main.call("tickets.queue", { id, urgency: "today", reason: "new" });
  }), WRITE_BUDGET);
  row(sheet, "reorder: a raise to now, then whenever kept", await timed(SAMPLES, (i) => main.call("tickets.queue", { id: ids[3 * i + 0 < entries ? 3 * i : i], urgency: "now", reason: "sooner" })), WRITE_BUDGET);
  row(sheet, "unqueue", await timed(Math.min(SAMPLES, entries >> 1), (i) => main.call("tickets.unqueue", { id: ids[entries - 1 - i] })), WRITE_BUDGET);

  // A read during a burst of writes.
  console.error("  burst");
  let stop = false, wrote = 0;
  const burst = (async () => {
    await Promise.all(Array.from({ length: 32 }, async (_, k) => {
      while (!stop) {
        const a = open[(1000 + wrote++ + k) % open.length];
        await main.call("tickets.inputs.ask", { ticket: a.ticket, question: "Under the burst?" });
      }
    }));
  })();
  const during = [], during2 = [];
  const end = Date.now() + BURST_MS;
  while (Date.now() < end) {
    let t = performance.now();
    await reader.call("tickets.queued", { limit: 50 });
    during.push(performance.now() - t);
    t = performance.now();
    await reader.call("tickets.inputs.list", { state: "open", limit: 50 });
    during2.push(performance.now() - t);
  }
  stop = true;
  await burst;
  facts.burst_writes_per_s = wrote / (BURST_MS / 1000);
  row(sheet, `queued 50 under a burst (${Math.round(facts.burst_writes_per_s)} writes/s)`, during, READ_BUDGET);
  row(sheet, "inputs list 50 under the burst", during2, READ_BUDGET);

  // Fan-out: 100 watches.
  for (const [label, params] of [["100 plain watches", {}], ["100 watches of `queued`", { filter: "queued", membership: true }]]) {
    console.error(`  fan-out, ${label}`);
    const sockets = await Promise.all(Array.from({ length: 100 }, () => Client.open(server.ws, token)));
    const seen = new Map();
    const marker = `fan-${label.length}-`;
    const streams = sockets.map((s) => s.stream("tickets.watch", params, (e) => {
      if (e?.type === "input_changed" || e?.type === "entered" || e?.type === "left") {
        const w = seen.get(e?.input?.question) ?? seen.get(e?.id) ?? seen.get(e?.ticket?.id ?? e?.ticket) ?? seen.get("any");
        if (w) {
          w.n += 1;
          if (w.n >= 100) w.done(performance.now());
        }
      }
    }));
    await sleep(500);
    const fan = [], own = [];
    for (let i = 0; i < 100; i++) {
      const q = `${marker}${i}?`;
      const a = open[(2000 + i) % open.length];
      const got = new Promise((done) => {
        const w = { n: 0, done };
        seen.set(q, w);
        seen.set("any", w);
      });
      const t = performance.now();
      // A plain watch hears a question; a watch of `queued`, a ticket leaving it.
      const writing = (params.filter
        ? main.call("tickets.unqueue", { id: ids[entries - 600 - i] })
        : main.call("tickets.inputs.ask", { ticket: a.ticket, question: q })
      ).then(() => own.push(performance.now() - t));
      const last = await Promise.race([got, sleep(2000).then(() => null)]);
      await writing;
      seen.clear();
      if (last != null) fan.push(last - t);
    }
    row(sheet, `a write beside ${label}`, own, WRITE_BUDGET);
    row(sheet, `its fan-out to the last of ${label}`, fan);
    streams.forEach((s) => s.close());
    sockets.forEach((s) => s.close());
    await sleep(300);
  }

  // 1,000 asks in parallel.
  console.error("  parallel");
  const par = async (sockets, n) => {
    const times = [];
    const t0 = performance.now();
    await Promise.all(Array.from({ length: n }, async (_, i) => {
      const t = performance.now();
      const s = sockets[i % sockets.length];
      await s.call("tickets.inputs.ask", { ticket: ids[(5000 + i) % ids.length], question: `In parallel ${i}?` });
      times.push(performance.now() - t);
    }));
    return { times, wall: performance.now() - t0 };
  };
  const one = await par([main], 1000);
  row(sheet, "1,000 asks in parallel over one socket, each", one.times);
  facts.par_one_wall_ms = one.wall;
  const many = await Promise.all(Array.from({ length: 100 }, () => Client.open(server.ws, token)));
  const hundred = await par(many, 1000);
  row(sheet, "1,000 asks in parallel over 100 sockets, each", hundred.times);
  facts.par_many_wall_ms = hundred.wall;
  many.forEach((s) => s.close());

  // 200 await_reply held.
  console.error("  await_reply held by 200");
  const waiters = await Promise.all(Array.from({ length: 200 }, () => Client.open(server.ws, token)));
  const replied = new Map();
  const held = waiters.map((s, i) => {
    const a = open[3000 + i];
    return s.stream("tickets.await_reply", { id: a.ticket, input: a.id }, (e) => {
      if (e?.type === "replied") replied.get(i)?.(performance.now());
    });
  });
  await sleep(1000);
  row(sheet, "ask beside 200 await_reply held", await timed(Math.min(SAMPLES, 200), () => main.call("tickets.inputs.ask", { ticket: pick(), question: "Beside the waits?" })), WRITE_BUDGET);
  const wake = [], woke = [];
  for (let i = 0; i < 200; i++) {
    const a = open[3000 + i];
    const got = new Promise((r) => replied.set(i, r));
    const t = performance.now();
    await main.call("tickets.inputs.answer", { ticket: a.ticket, input: a.id, value: { type: "text", text: "answered" } });
    woke.push(performance.now() - t);
    const at = await Promise.race([got, sleep(5000).then(() => null)]);
    if (at != null) wake.push(at - t);
  }
  row(sheet, "answer beside the waits", woke, WRITE_BUDGET);
  row(sheet, "an answer to its wait's reply", wake);
  held.forEach((s) => s.close());
  waiters.forEach((s) => s.close());

  // Page.
  if (flag("--page")) {
    console.error("  page");
    const { chromium } = await playwright();
    const v = bundle(work, "ui", { engine: built?.engine });
    const answer = push(server, sessionFile, signerFile, v);
    if (answer?.type !== "pushed") throw new Error(`push: ${JSON.stringify(answer)}`);
    const browser = await chromium.launch({ headless: true });
    const firsts = [], creates = [];
    try {
      for (let i = 0; i < 6; i++) {
        const context = await browser.newContext();
        await context.addCookies(sessionCookies(server.origin, token));
        await context.addInitScript(INIT);
        await context.addInitScript(() => {
          window.__queued = {};
          const Native = window.WebSocket;
          window.WebSocket = class extends Native {
            constructor(...a) {
              super(...a);
              this.addEventListener("message", (e) => {
                if (typeof e.data !== "string" || !e.data.includes('"queued"')) return;
                const m = e.data.includes('"type":"queued"') ? /"entry":\{[^}]*?"ticket":"([^"]+)"/.exec(e.data) : null;
                if (m && !(m[1] in window.__queued)) window.__queued[m[1]] = performance.now();
              });
            }
          };
        });
        const page = await context.newPage();
        await page.goto(`${server.origin}/app/#/queue`);
        await page.locator(".q-row [data-field=title]").first().waitFor({ timeout: 30000 });
        const first = await page.evaluate(() => Math.min(...Object.values(window.__pages.rows)));
        if (i > 0) firsts.push(first);
        await sleep(300);
        const id = fresh[100 + i] ?? ids[i + 40];
        await main.call("tickets.queue", { id, urgency: "now", reason: "the page" });
        const until = Date.now() + 5000;
        while (Date.now() < until) {
          const t = await page.evaluate((id) => ({ frame: window.__queued[id], row: window.__pages.rows[id] }), id);
          if (t.frame != null && t.row != null) {
            if (i > 0) creates.push(t.row - t.frame);
            break;
          }
          await sleep(10);
        }
        await context.close();
      }
    } finally {
      await browser.close();
    }
    row(sheet, "the page: #/queue's first row, cold", firsts, PAGE_BUDGET);
    row(sheet, "the page: a queued row's frame to its row", creates);
  }

  // Restart: all of it, killed, up again.
  console.error("  restart");
  main.close();
  reader.close();
  const rssFull = rssKb(server.child.pid);
  server.stop();
  await sleep(300);
  const at = Date.now();
  const again = spawn(path.join(TARGET, "release", "menger"), [
    "--port", String(server.ws), "--shell-port", String(server.shell), "--auth-provider", "key", "--admin", principal,
    "--db-path", path.join(work, "tickets.db"), "--data-dir", work, "--ui-db-path", path.join(work, "ui.db"), "--ui-signer", signer,
  ], { env: { ...process.env, PLEXUS_NO_REGISTRY: "1" }, stdio: "ignore" });
  process.once("exit", () => again.kill("SIGKILL"));
  let served = null;
  while (Date.now() - at < 600000 && !served) {
    try {
      const c = await Client.open(server.ws, token);
      await c.call("tickets.health");
      served = Date.now() - at;
      c.close();
    } catch {
      await sleep(20);
    }
  }
  facts.restart_ms = served;
  await sleep(500);
  const check = await Client.open(server.ws, token);
  const end2 = (await check.call("tickets.queued", { limit: 1 })).find((e) => e.type === "queue_end");
  facts.after_restart = { count: end2?.count };
  check.close();
  facts.rss_restart_kb = rssKb(again.pid);
  facts.rss_full_kb = rssFull;
  again.kill("SIGKILL");
  console.error(`  restart to first answer ${served} ms; ${end2?.count} entries; RSS ${(rssFull / 1024).toFixed(0)} MB → ${(facts.rss_restart_kb / 1024).toFixed(0)} MB; ${facts.bytes_per_item.toFixed(0)} B an item`);
  facts.work = work;
  return { exp: N, sheet, facts };
}

const main = async () => {
  if (!flag("--no-build")) build("release");
  const out = [];
  for (const e of RUNGS) out.push(await rung(e));
  console.log("| tickets | what | n | p50 ms | p99 ms | max ms | budget |");
  console.log("|---|---|---|---|---|---|---|");
  for (const r of out) {
    for (const s of r.sheet) console.log(`| ${r.exp} | ${s.what} | ${s.n} | ${ms(s.p50)} | ${ms(s.p99)} | ${ms(s.max)} | ${s.budget ?? ""}${s.over ? " **over**" : ""} |`);
  }
  for (const r of out) console.log(JSON.stringify({ tickets: r.exp, facts: r.facts }));
  process.exit(out.some((r) => r.sheet.some((s) => s.over)) ? 1 : 0);
};
main().catch((e) => {
  console.error(e);
  process.exit(2);
});
