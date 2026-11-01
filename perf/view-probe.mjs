#!/usr/bin/env node
// SURFACE-16: what a ticket-view costs against today's ticket-body.
//
//   node perf/view-probe.mjs [--view 1,100,1000] [--body 1,100,200,250,300] [--runs 3] [--no-build] [--headed]
//
// A throwaway menger (scripts/ui-throwaway.mjs) with 1,000 tickets of about 2 KB
// of markdown, and ui/ pushed with four pages only this probe's bundle holds:
//
//   view   one `tickets.list` (a one-shot `live`), a sibling-imported ticket-view per row
//   mark   the same list, a bound mark per row (`plexus-ui:ticket-view`, the body as `data-body`)
//   body   N sibling-imported ticket-bodies with distinct ids: its own `get`, its own `watch`
//   none   the same page with nothing on it: what the others are measured above
//
// Each load a fresh browser context. Per instance: the JS heap after a
// collection (CDP Runtime.getHeapUsage), the engine's wasm memory, and DOM nodes
// (CDP Memory.getDOMCounters), each over `none`. All painted: on the page's
// clock, navigation start to the frame after the last `[data-ticket-body]` is
// in the DOM (a frame polls for it); past the fold nothing is rastered, its
// layout is done. The wire: every `.call` the page sent, by method, and
// every refusal (`too_many_calls`), once the frames have gone quiet for 2 s (15 s past a refusal).
// The median of the runs; what was drawn is the last run's.
import fs from "node:fs";
import path from "node:path";
import { spawn, execSync } from "node:child_process";
import { gzipSync } from "node:zlib";
import { makeKey, readToken, sessionCookies, writeSession } from "../scripts/throwaway-login.mjs";
import { INIT, REPO, Throwaway, build, bundle, playwright, push, sentCalls, sleep, synapse, workDir } from "../scripts/ui-throwaway.mjs";

const args = process.argv.slice(2);
const opt = (n, d) => (args.includes(n) ? args[args.indexOf(n) + 1] : d);
const flag = (n) => args.includes(n);
const list = (n, d) => opt(n, d).split(",").map(Number);
const VIEW = list("--view", "1,100,1000");
const BODY = list("--body", "1,100,200,250,300");
const RUNS = Number(opt("--runs", "3"));
const TICKETS = Math.max(...VIEW, ...BODY, 1);
const median = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];

/** About 2 KB of what a ticket's body holds: headings, prose, a list, code, a table. */
function markdown(i) {
  const para = `The kiln's firing log for batch ${i} keeps each schedule beside what came out of it, so the next load starts from what the last one taught. A [schedule](https://example.com/s/${i}) is a program whose steps are temperatures, and \`hold_at(${i % 7})\` is the step this ticket is about.`;
  return `## Problem

${para}

${para}

## Scope

- the **bisque** shelf, cone 04, loaded by hand
- the *glaze* shelf, cone 6, with witness cones on each level
- a log line per segment, written as it ends
- nothing about the gas train: that is its own ticket

## What it does

\`\`\`rust
fn fire(schedule: &Schedule, log: &mut Log) -> Result<Firing, Fault> {
    for step in schedule.steps() {
        let reached = kiln.ramp_to(step.target, step.rate)?;
        log.segment(step, reached);
    }
    Ok(Firing::done(${i}))
}
\`\`\`

| cone | target | rate |
|---|---|---|
| 04 | 1060 C | 110 C/h |
| 6 | 1222 C | 60 C/h |

> A firing that stops early says which segment, and why.

## Done when

${para}

${para}

${para}

1. the schedule is stored with the batch
2. the log reads back in order
3. an aborted firing names its segment
`;
}

const page = (script, markup) => `<script module>\n  export const uses = [];\n</script>\n<script>\n${script}\n</script>\n${markup}\n`;
const LIST = `  import { live } from "plexus-ui/host";
  let { root, n } = $props();
  const tickets = live({ snapshot: "tickets.list", params: () => ({ parent: root, limit: Number(n) }), key: "id" });`;
const SOURCES = {
  "probe-none": page(`  let { root, n } = $props();`, `<main data-probe="none"></main>`),
  "probe-view": page(`${LIST}\n  import TicketView from "./ticket-view.svelte";`,
    `<main data-probe="view">{#each tickets.rows as t (t.id)}<TicketView body={t.body} title={t.id + " " + t.title} status={t.status}/>{/each}</main>`),
  "probe-mark": page(`${LIST}\n  import TicketView from "plexus-ui:ticket-view";`,
    `<main data-probe="mark">{#each tickets.rows as t (t.id)}<TicketView body={t.body} title={t.id + " " + t.title} status={t.status}/>{/each}</main>`),
  "probe-body": page(`  import TicketBody from "./ticket-body.svelte";
  let { root, n } = $props();
  // root: "SHORT-first", the first of n consecutive ids.
  const [short, first] = root.split("-");
  const ids = Array.from({ length: Number(n) }, (_, i) => \`\${short}-\${Number(first) + i}\`);`,
    `<main data-probe="body">{#each ids as id (id)}<TicketBody {id}/>{/each}</main>`),
};
const declared = (name, src) => {
  const uses = { "probe-view": ["tickets.list"], "probe-mark": ["tickets.list"] }[name] ?? [];
  return src.replace("export const uses = [];", `export const uses = ${JSON.stringify(uses)};`);
};
function addPages(ui) {
  const routes = JSON.parse(fs.readFileSync(path.join(ui, "routes.json"), "utf8"));
  for (const [name, src] of Object.entries(SOURCES)) {
    fs.writeFileSync(path.join(ui, "src", `${name}.svelte`), declared(name, src));
    routes[`#/${name}/:root/:n`] = name;
  }
  fs.writeFileSync(path.join(ui, "routes.json"), JSON.stringify(routes, null, 2));
}

const size = (tar, f) => {
  const b = execSync(`tar -xOf ${tar} ${f}`, { maxBuffer: 1 << 28 });
  return { raw: b.length, gz: gzipSync(b).length };
};

async function pool(n, jobs) {
  let at = 0;
  await Promise.all(Array.from({ length: n }, async () => { while (at < jobs.length) await jobs[at++](); }));
}

async function main() {
  const work = workDir("ui8-view");
  console.error(`work: ${work}`);
  if (!flag("--no-build")) build("release");
  const b = bundle(work, "probe", { edit: addPages });
  const old = bundle(work, "before", {
    engine: b.engine,
    edit: (ui) => {
      fs.writeFileSync(path.join(ui, "src", "ticket-body.svelte"), execSync(`git -C ${REPO} show fc2d27f3:ui/src/ticket-body.svelte`));
      fs.rmSync(path.join(ui, "src", "ticket-view.svelte"));
      const sheet = path.join(ui, "src", "sheet-page.svelte");
      fs.writeFileSync(sheet, fs.readFileSync(sheet, "utf8").split("\n").filter((l) => !l.includes("TicketView")).join("\n"));
    },
  });
  const modules = {
    "ticket-view.js": size(b.tar, "components/ticket-view.js"),
    "ticket-body.js": size(b.tar, "components/ticket-body.js"),
    "ticket-body.js before": size(old.tar, "components/ticket-body.js"),
    "probe-view.js (a list, the view inlined)": size(b.tar, "components/probe-view.js"),
    "probe-mark.js (a list, the view by mark)": size(b.tar, "components/probe-mark.js"),
    "svelte-runtime.js": size(b.tar, "components/svelte-runtime.js"),
  };
  console.log(JSON.stringify({ modules }));

  const adminKey = path.join(work, "admin.key");
  const signerKey = path.join(work, "signer.key");
  const admin = makeKey(adminKey);
  const server = new Throwaway(work, "release");
  let browser;
  try {
    await server.start(admin, makeKey(signerKey).replace(/^key:/, ""), 100);
    const session = path.join(work, "session");
    await writeSession(`ws://127.0.0.1:${server.ws}`, adminKey, session, "owner");
    const answer = push(server, session, signerKey, b);
    if (answer?.type !== "pushed") throw new Error(`ui push: ${JSON.stringify(answer)}`);
    const sy = (...a) => synapse(server, session, ...a);

    // The tickets, children of one scope, ids consecutive.
    const [root] = sy("tickets", "create", "--epic", "AAAA", "--kind", "scope", "--title", "the probe's bodies");
    if (root?.type !== "created") throw new Error(`the scope: ${JSON.stringify(root)}`);
    const seq = Number(root.ticket.id.split("-")[1]);
    const dir = path.join(work, "bodies");
    fs.mkdirSync(dir);
    const env = { ...process.env, XDG_CACHE_HOME: path.join(work, "cache") };
    const started = Date.now();
    await pool(6, Array.from({ length: TICKETS }, (_, i) => () => new Promise((done, fail) => {
      // One at a time would be a quarter of an hour on a loaded machine.
      const f = path.join(dir, `${i}.md`);
      fs.writeFileSync(f, markdown(i));
      const c = spawn("synapse", ["--no-hold", "--token-file", session, "-P", String(server.ws), "menger", "tickets", "create", "--epic", "AAAA", "--parent", root.ticket.id, "--title", `a firing log, batch ${i}`, "--body", `@${f}`], { env, stdio: "ignore" });
      c.on("exit", (code) => (code === 0 ? done() : fail(new Error(`create ${i}: exit ${code}`))));
    })));
    console.error(`${TICKETS} tickets of ${markdown(1).length} B in ${((Date.now() - started) / 1000).toFixed(0)} s`);
    const first = `AAAA-${seq + 1}`;
    const [check] = sy("tickets", "get", "--id", `AAAA-${seq + TICKETS}`);
    if (!check?.ticket?.body) throw new Error(`the last ticket: ${JSON.stringify(check)}`);

    const { chromium } = await playwright();
    browser = await chromium.launch({ headless: !flag("--headed") });
    const token = readToken(session);

    /** One cold load of `kind` at `n`. */
    async function load(kind, n) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      await context.addCookies(sessionCookies(server.origin, token));
      await context.addInitScript(INIT);
      await context.addInitScript((want) => {
        const up = () => {
          if (document.querySelectorAll("[data-ticket-body]").length >= want) {
            requestAnimationFrame(() => (window.__drawn = performance.now()));
            return;
          }
          requestAnimationFrame(up);
        };
        if (want > 0) requestAnimationFrame(up);
      }, n);
      const p = await context.newPage();
      const cdp = await context.newCDPSession(p);
      const asked = new Map(), stream = new Map(), refusedBy = {};
      let refused = 0, lastFrame = Date.now(), received = 0;
      const console_ = [];
      p.on("console", (m) => console_.push(m.text()));
      p.on("websocket", (ws) => {
        const hear = () => { lastFrame = Date.now(); };
        ws.on("framesent", (f) => {
          hear();
          try {
            const j = JSON.parse(f.payload);
            if (j.method?.endsWith(".call")) asked.set(j.id, j.params?.method);
          } catch { /* not a call */ }
        });
        ws.on("framereceived", (f) => {
          hear();
          const d = typeof f.payload === "string" ? f.payload : "";
          received += d.length;
          try {
            const j = JSON.parse(d);
            if (asked.has(j.id) && j.result != null) stream.set(j.result, asked.get(j.id)); // the ack: the call's subscription
            if (d.includes("too_many_calls")) {
              refused++;
              const m = stream.get(j.params?.subscription) ?? "?";
              refusedBy[m] = (refusedBy[m] ?? 0) + 1;
            }
          } catch { /* not a frame we read */ }
        });
      });
      const root_ = kind === "body" ? first : root.ticket.id;
      await p.goto(`${server.origin}/app/#/probe-${kind}/${root_}/${n}`);
      await p.waitForSelector(`[data-probe=${kind}]`, { state: "attached", timeout: 60_000 });
      const until = Date.now() + 90_000;
      let drawn = null;
      while (n > 0 && Date.now() < until && drawn == null) {
        drawn = await p.evaluate(() => window.__drawn ?? null);
        if (Date.now() - lastFrame > 5_000 && (await p.evaluate(() => document.querySelectorAll("[data-ticket-body]").length)) < n) break;
        await sleep(50);
      }
      // A refusal's retry, if any, may come late: past one, 15 s of quiet.
      while (Date.now() - lastFrame < (refused ? 15_000 : 2_000) && Date.now() < until + 30_000) await sleep(100);
      await sleep(n > 0 ? 500 : 1_500);
      const calls = {};
      for (const c of await sentCalls(p)) calls[c.method] = (calls[c.method] ?? 0) + 1;
      await cdp.send("HeapProfiler.collectGarbage");
      await sleep(100);
      await cdp.send("HeapProfiler.collectGarbage");
      const heap = (await cdp.send("Runtime.getHeapUsage")).usedSize;
      await cdp.send("Memory.enable").catch(() => {});
      const nodes = (await cdp.send("Memory.getDOMCounters")).nodes;
      const seen = await p.evaluate(() => ({ bodies: document.querySelectorAll("[data-ticket-body]").length, surfaces: document.querySelectorAll("[data-surface=ticket-body]").length, wasm: window.__pages.memory?.buffer?.byteLength ?? null, elements: document.querySelectorAll("*").length }));
      await context.close();
      return { refusedBy, kind, n, drawn, heap, nodes, calls, refused, received, ...seen, console: [...new Set(console_)].slice(0, 6) };
    }

    const measure = async (kind, n) => {
      const runs = [];
      for (let i = 0; i < RUNS; i++) runs.push(await load(kind, n));
      const m = (k) => median(runs.map((r) => r[k] ?? 0));
      const last = runs.at(-1);
      return { kind, n, drawn: runs.every((r) => r.drawn == null) ? null : m("drawn"), times: runs.map((r) => r.drawn && Math.round(r.drawn)), heap: m("heap"), nodes: m("nodes"), wasm: m("wasm"), elements: m("elements"), bodies: last.bodies, refused: runs.map((r) => r.refused), calls: last.calls, received: last.received, console: last.console, refusedBy: last.refusedBy };
    };
    await load("none", 0); // the browser's own first start, not counted
    const none = await measure("none", 0);
    console.log(JSON.stringify({ none }));
    const rows = [];
    for (const [kind, ns] of [["view", VIEW], ["mark", VIEW], ["body", BODY]]) {
      for (const n of ns) {
        const r = await measure(kind, n);
        const per = (k) => (r[k] - none[k]) / n;
        const out = { ...r, perHeapKiB: +(per("heap") / 1024).toFixed(2), perWasmKiB: +(per("wasm") / 1024).toFixed(2), perNodes: +per("nodes").toFixed(1), perElements: +per("elements").toFixed(1) };
        rows.push(out);
        console.log(JSON.stringify(out));
      }
    }
  } finally {
    await browser?.close();
    server.stop();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
