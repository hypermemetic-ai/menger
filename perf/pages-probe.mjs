#!/usr/bin/env node
// UI-8: the ladder's browser rows for v2 — the first pages on the engine.
//
//   node perf/pages-probe.mjs [--seed N] [--runs 5] [--timeout-ms 30000] [--debug] [--no-build] [--headed] [--keep]
//
// A throwaway menger seeded N (default 10⁶; release build), the bundle
// from scripts/ui-build.sh pushed to it (scripts/ui-throwaway.mjs, as
// tests/pages_browser.mjs does), then `--runs` cold loads of /app/, each
// in a fresh browser context signed in by cookie:
//
//   board first rows painted   navigation start (the page's own time origin)
//                              to the first `.board-row[data-id]` in the DOM,
//                              stamped by a MutationObserver installed
//                              before the engine (INIT); the median of the runs
//   wasm memory                the engine's linear memory once the first rows
//                              are on screen (INIT keeps the module's exports
//                              as wasm-bindgen instantiates it); the median
//   rows held                  `.board-row`s in the DOM at that moment
//
// Budgets (UI-8's `first_rows_at_a_million`): first rows ≤ 0.3 s, wasm
// memory ≤ 10 MB. Prints one row the way perf/notes.md records them, and
// a JSON line after it; exits 1 when a budget is missed, 2 when setup failed.
import path from "node:path";
import { makeKey, readToken, sessionCookies, writeSession } from "../scripts/throwaway-login.mjs";
import { INIT, Throwaway, build, bundle, playwright, push, sleep, workDir } from "../scripts/ui-throwaway.mjs";

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const opt = (name, dflt) => (args.includes(name) ? args[args.indexOf(name) + 1] : dflt);
const SEED = Number(opt("--seed", "1000000"));
const RUNS = Number(opt("--runs", "5"));
const PROFILE = flag("--debug") ? "debug" : "release";
const FIRST_ROWS_MS = 300;
const MEMORY_BYTES = 10 * 1000 * 1000;
const RUN_TIMEOUT_MS = Number(opt("--timeout-ms", "30000"));

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
  console.error(`work: ${work} · seed ${SEED} · ${RUNS} runs · ${PROFILE}`);
  if (!flag("--no-build")) build(PROFILE);
  const b = bundle(work, "probe");
  const adminKey = path.join(work, "admin.key");
  const signerKey = path.join(work, "signer.key");
  const admin = makeKey(adminKey);
  const server = new Throwaway(work, PROFILE);
  const t0 = Date.now();
  await server.start(admin, makeKey(signerKey).replace(/^key:/, ""), SEED);
  console.error(`server up, ${SEED} seeded, in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  const session = path.join(work, "session");
  await writeSession(`ws://127.0.0.1:${server.ws}`, adminKey, session, "owner");
  const answer = push(server, session, signerKey, b);
  if (answer?.type !== "pushed") throw new Error(`ui push: ${JSON.stringify(answer)}`);
  const token = readToken(session);

  const { chromium } = await playwright();
  const browser = await chromium.launch({ headless: !flag("--headed") });
  const runs = [];
  try {
    // One warm-up load, not counted (the browser's own first start).
    for (let i = 0; i <= RUNS; i++) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      await context.addCookies(sessionCookies(server.origin, token));
      await context.addInitScript(INIT);
      const page = await context.newPage();
      await page.goto(`${server.origin}/app/`);
      const deadline = Date.now() + RUN_TIMEOUT_MS;
      let got = null;
      while (Date.now() < deadline) {
        got = await page.evaluate(() => {
          const P = window.__pages;
          if (P.firstRow == null) return null;
          return { firstRow: P.firstRow, memory: P.memory?.buffer?.byteLength ?? null, held: document.querySelectorAll(".board-row[data-id]").length };
        });
        if (got) break;
        await sleep(20);
      }
      got ??= { firstRow: null, held: 0, memory: await page.evaluate(() => window.__pages.memory?.buffer?.byteLength ?? null) };
      if (i > 0) runs.push(got);
      await context.close();
    }
  } finally {
    await browser.close();
    if (!flag("--keep")) server.stop();
    else console.error(`kept: ws ${server.ws}, shell ${server.origin} (pid ${server.child.pid})`);
  }

  const painted = runs.filter((r) => r.firstRow != null);
  const first = median(painted.map((r) => r.firstRow));
  const memory = median(runs.filter((r) => r.memory != null).map((r) => r.memory));
  const held = median(painted.map((r) => r.held));
  const each = runs.map((r) => (r.firstRow == null ? "never" : `${r.firstRow.toFixed(0)}`)).join(", ");
  const ok = painted.length === runs.length && first <= FIRST_ROWS_MS && memory <= MEMORY_BYTES;
  console.log(
    `| ${magnitude(SEED)} | v2 · board first rows painted | ${painted.length ? `${(first / 1000).toFixed(3)} s` : "never"} (median of ${RUNS}: ${each} ms) | wasm ${Number.isFinite(memory) ? mib(memory) : "unknown"} · ${Number.isFinite(held) ? held : 0} rows held | ${ok ? "within" : "over"} 0.3 s / 10 MB |`,
  );
  console.log(JSON.stringify({ seed: SEED, runs, first_rows_ms: first, wasm_bytes: memory, rows_held: held, ok }));
  process.exit(ok ? 0 : 1);
}

main().catch((e) => {
  console.error(`setup failed: ${e.stack || e.message}`);
  process.exit(2);
});
