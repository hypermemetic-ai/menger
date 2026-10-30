#!/usr/bin/env node
// UI-8: the ladder's browser rows for v2 — the first pages on the engine.
//
//   node perf/pages-probe.mjs [--seed N] [--runs 5] [--path /app/] [--timeout-ms 30000] [--debug] [--no-build] [--headed] [--keep]
//
// `--path` (FRAME-14) is what each load opens, the board by its route:
// `/app/` (the default) the rhx board, `/app/#/sv/` the Svelte one
// (svelte/1). Both draw the same marks, so one probe reads either.
//
// A throwaway menger seeded N (default 10⁶; release build), the bundle
// from plexus-ui build pushed to it (scripts/ui-throwaway.mjs, as
// tests/pages_browser.mjs does), then `--runs` cold loads of `--path`, each
// in a fresh browser context signed in by cookie:
//
//   board first rows painted   navigation start (the page's own time origin)
//                              to the first `.board-row[data-id]` showing its
//                              title (`[data-field=title]`, from `list`),
//                              stamped by a MutationObserver installed
//                              before the engine (INIT); the median of the runs
//   wasm memory                the engine's linear memory about a second after
//                              the first rows are on screen (INIT keeps the module's exports
//                              as wasm-bindgen instantiates it); the median
//   rows held                  `.board-row`s in the DOM at that moment
//   JS heap                    (FRAME-14) the page's JS heap in use at that
//                              moment, after a collection (CDP
//                              `Runtime.getHeapUsage`; the wasm's memory is
//                              apart): what a svelte/1 board costs; the median
//   created row                (FRAME-14) a ticket made by synapse after it,
//                              first in the store's order: its `created`
//                              frame to its `.board-row` (INIT's
//                              `__pages.created` and `__pages.boardRows`); the median
//   marks                      FRAME-17: `tickets.list` sent (INIT's frames) and every mark the host
//                              sets on the page's clock (`plexus-ui:*`, CONTRACT.md's host chapter),
//                              each one's median, on a line of their own and in the JSON
//
// Budgets (UI-8's `first_rows_at_a_million`): first rows ≤ 0.3 s, wasm
// memory ≤ 10 MB. Prints one row the way perf/notes.md records them, and
// a JSON line after it; exits 1 when a budget is missed, 2 when setup failed.
import path from "node:path";
import { makeKey, readToken, sessionCookies, writeSession } from "../scripts/throwaway-login.mjs";
import { INIT, Throwaway, build, bundle, playwright, push, sentCalls, sleep, synapse, workDir } from "../scripts/ui-throwaway.mjs";

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const opt = (name, dflt) => (args.includes(name) ? args[args.indexOf(name) + 1] : dflt);
const SEED = Number(opt("--seed", "1000000"));
const RUNS = Number(opt("--runs", "5"));
const PROFILE = flag("--debug") ? "debug" : "release";
const FIRST_ROWS_MS = 300;
const MEMORY_BYTES = 10 * 1000 * 1000;
const RUN_TIMEOUT_MS = Number(opt("--timeout-ms", "30000"));
const PATH = opt("--path", "/app/");

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s.length ? (s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2) : NaN;
};
const magnitude = (n) => {
  const e = Math.log10(n);
  return Number.isInteger(e) ? `10${String(e).replace(/./g, (d) => "⁰¹²³⁴⁵⁶⁷⁸⁹"[d])}` : n.toLocaleString("en");
};
const mib = (b) => `${(b / 1024 / 1024).toFixed(1)} MiB`;

async function main() {
  const work = workDir("ui8-probe");
  console.error(`work: ${work} · seed ${SEED} · ${RUNS} runs · ${PROFILE} · ${PATH}`);
  if (!flag("--no-build")) build(PROFILE);
  const b = bundle(work, "probe");
  const adminKey = path.join(work, "admin.key");
  const signerKey = path.join(work, "signer.key");
  const admin = makeKey(adminKey);
  const server = new Throwaway(work, PROFILE);
  server.keep = flag("--keep");
  const runs = [];
  let browser;
  // FRAME-14: from the start on, a failure anywhere stops the server.
  try {
    const t0 = Date.now();
    await server.start(admin, makeKey(signerKey).replace(/^key:/, ""), SEED);
    console.error(`server up, ${SEED} seeded, in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
    const session = path.join(work, "session");
    await writeSession(`ws://127.0.0.1:${server.ws}`, adminKey, session, "owner");
    const answer = push(server, session, signerKey, b);
    if (answer?.type !== "pushed") throw new Error(`ui push: ${JSON.stringify(answer)}`);
    const token = readToken(session);
    const sy = (...a) => synapse(server, session, ...a);
    // The created rows' shortcode, first in the store's order: its tickets are the board's first rows.
    const [root] = sy("tickets", "create", "--epic", "AAAA", "--kind", "scope", "--title", "the probe's created rows");
    if (root?.type !== "created") throw new Error(`the probe's scope: ${JSON.stringify(root)}`);

    const { chromium } = await playwright();
    browser = await chromium.launch({ headless: !flag("--headed") });
    // One warm-up load, not counted (the browser's own first start).
    for (let i = 0; i <= RUNS; i++) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      await context.addCookies(sessionCookies(server.origin, token));
      await context.addInitScript(INIT);
      const page = await context.newPage();
      const cdp = await context.newCDPSession(page);
      await page.goto(`${server.origin}${PATH}`);
      const deadline = Date.now() + RUN_TIMEOUT_MS;
      let got = null;
      while (Date.now() < deadline) {
        got = await page.evaluate(() => {
          const P = window.__pages;
          if (P.firstRow == null) return null;
          return { firstRow: P.firstRow, memory: null, heap: null, created: null, held: document.querySelectorAll(".board-row[data-id]").length };
        });
        if (got) break;
        await sleep(20);
      }
      // FRAME-17: the board's list sent, and the host's own marks.
      const marks = got ? await page.evaluate(() => { const m = {}; for (const e of performance.getEntriesByType("mark")) if (e.name.startsWith("plexus-ui:")) m[e.name] ??= e.startTime; return m; }) : {};
      const list = got ? (await sentCalls(page)).find((c) => c.method === "tickets.list") : null;
      if (list) marks["tickets.list sent"] = list.t;
      // The memory a second after the first rows: what the page holds once settled.
      const heap = async () => {
        await cdp.send("HeapProfiler.collectGarbage").catch(() => {});
        return (await cdp.send("Runtime.getHeapUsage").catch(() => null))?.usedSize ?? null;
      };
      if (got) {
        await sleep(1_000);
        got.memory = await page.evaluate(() => window.__pages.memory?.buffer?.byteLength ?? null);
        got.heap = await heap();
        // A created ticket: its frame → its row.
        const [made] = sy("tickets", "create", "--epic", "AAAA", "--parent", root.ticket.id, "--title", `made by the probe, run ${i}`);
        const id = made?.ticket?.id;
        const until = Date.now() + 5_000;
        while (id && Date.now() < until) {
          const t = await page.evaluate((id) => ({ frame: window.__pages.created[id], row: window.__pages.boardRows[id] }), id);
          if (t.frame != null && t.row != null) {
            got.created = t.row - t.frame;
            break;
          }
          await sleep(20);
        }
      }
      got ??= { firstRow: null, held: 0, heap: await heap(), created: null, memory: await page.evaluate(() => window.__pages.memory?.buffer?.byteLength ?? null) };
      if (i > 0) runs.push({ ...got, marks });
      await context.close();
    }
  } finally {
    await browser?.close();
    if (!server.keep) server.stop();
    else console.error(`kept: ws ${server.ws}, shell ${server.origin} (pid ${server.child?.pid})`);
  }

  const painted = runs.filter((r) => r.firstRow != null);
  const first = median(painted.map((r) => r.firstRow));
  const memory = median(runs.filter((r) => r.memory != null).map((r) => r.memory));
  const held = median(painted.map((r) => r.held));
  const heap = median(runs.filter((r) => r.heap != null).map((r) => r.heap));
  const createdMs = median(runs.filter((r) => r.created != null).map((r) => r.created));
  const each = runs.map((r) => (r.firstRow == null ? "never" : `${r.firstRow.toFixed(0)}`)).join(", ");
  const ok = painted.length === runs.length && first <= FIRST_ROWS_MS && memory <= MEMORY_BYTES;
  console.log(
    `| ${magnitude(SEED)} | v2 · board first rows painted${PATH === "/app/" ? "" : ` · ${PATH}`} | ${painted.length ? `${(first / 1000).toFixed(3)} s` : "never"} (median of ${RUNS}: ${each} ms) | wasm ${Number.isFinite(memory) ? mib(memory) : "unknown"} · ${Number.isFinite(held) ? held : 0} rows held · JS heap ${Number.isFinite(heap) ? mib(heap) : "unknown"} · created row ${Number.isFinite(createdMs) ? `${createdMs.toFixed(1)} ms` : "never"} | ${ok ? "within" : "over"} 0.3 s / 10 MB |`,
  );
  const names = [...new Set(runs.flatMap((r) => Object.keys(r.marks ?? {})))];
  const marks = Object.fromEntries(names.map((n) => [n, median(runs.map((r) => r.marks?.[n]).filter((x) => x != null))]).sort((a, b) => a[1] - b[1]));
  console.log(`  marks (median, ms): ${Object.entries(marks).map(([n, t]) => `${n} +${t.toFixed(0)}`).join(" · ") || "none"}${names.some((n) => n.startsWith("plexus-ui:")) ? "" : " (the engine sets no plexus-ui:* marks)"}`);
  console.log(JSON.stringify({ seed: SEED, path: PATH, runs, first_rows_ms: first, wasm_bytes: memory, rows_held: held, js_heap_bytes: heap, created_row_ms: createdMs, marks, ok }));
  process.exit(ok ? 0 : 1);
}

main().catch((e) => {
  console.error(`setup failed: ${e.stack || e.message}`);
  process.exit(2);
});
