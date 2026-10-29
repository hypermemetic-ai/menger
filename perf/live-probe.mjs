// FRAME-16: the first row on the wire — cold loads of a deployed menger (app-test by default),
// each in a fresh browser context signed in by a session cookie read from a file (never argv):
//
//   node perf/live-probe.mjs --token-file FILE [--origin https://app-test.menger.sh] [--runs 5] [--path /]
//
// Prints: the first titled board row's median over the runs (navigation start → the first
// `.board-row[data-id] [data-field=title]`), TTFB, when the socket opened, and the waterfall by
// kind (requests, bytes on the wire and decoded, from → to) of the last run — the shape FRAME-16
// changes. A warm-up load runs first and is not counted. Playwright comes from perf/node_modules.
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, dflt) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : dflt; };
const tokenFile = opt("--token-file", null);
if (!tokenFile) { console.error("live-probe: --token-file FILE (a session token; never in argv)"); process.exit(2); }
const origin = opt("--origin", "https://app-test.menger.sh").replace(/\/$/, "");
const runs = Number(opt("--runs", "5"));
const route = opt("--path", "/");
const token = readFileSync(tokenFile, "utf8").trim();
const req = createRequire(path.join(here, "package.json"));
const { chromium } = await import(req.resolve("playwright").replace(/index\.js$/, "index.mjs"));
const cookieName = origin.startsWith("https://") ? "__Host-menger_session" : "menger_session";

const browser = await chromium.launch();
const firsts = [], ttfbs = [], sockets = [];
let last = null;
for (let i = 0; i <= runs; i++) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await ctx.addCookies([{ name: cookieName, value: token, url: origin, secure: origin.startsWith("https://"), httpOnly: true, sameSite: "Lax" }]);
  const page = await ctx.newPage();
  const opened = []; page.on("websocket", () => opened.push(Date.now()));
  const t0 = Date.now();
  await page.goto(origin + route, { waitUntil: "commit" });
  const ok = await page.waitForSelector(".board-row[data-id] [data-field=title]", { timeout: 30000 }).then(() => true).catch(() => false);
  const first = ok ? await page.evaluate(() => performance.now()) : null;
  const nav = await page.evaluate(() => { const n = performance.getEntriesByType("navigation")[0]; return { ttfb: n.responseStart, proto: n.nextHopProtocol }; });
  const res = await page.evaluate(() => performance.getEntriesByType("resource").map(r => ({ name: r.name.replace(location.origin, ""), start: r.startTime, dur: r.duration, enc: r.encodedBodySize, dec: r.decodedBodySize })));
  if (i > 0) { firsts.push(first); ttfbs.push(nav.ttfb); sockets.push(opened.length ? opened[0] - t0 : null); last = { res, nav, first }; }
  await ctx.close();
}
await browser.close();
const ms = (x) => (x == null ? "—" : `${Math.round(x)}`);
const median = (a) => { const b = a.filter(x => x != null).sort((x, y) => x - y); return b.length ? b[Math.floor(b.length / 2)] : null; };
console.log(`${origin}${route}: first titled row median ${ms(median(firsts))} ms (${firsts.map(ms).join(", ")}); TTFB median ${ms(median(ttfbs))} ms; socket opened at +${ms(median(sockets))} ms; ${last.nav.proto}`);
const kind = (n) => n.includes("engine.wasm") ? "engine.wasm" : n.includes("engine.js") ? "engine.js" : n.includes("host.js") ? "host.js" : n.includes("/components") ? "components" : n.includes("manifest") ? "manifest" : "other";
const by = {}; for (const r of last.res) (by[kind(r.name)] ||= []).push(r);
for (const [k, rs] of Object.entries(by)) {
  const enc = rs.reduce((a, r) => a + (r.enc || 0), 0), dec = rs.reduce((a, r) => a + (r.dec || 0), 0);
  const s = Math.min(...rs.map(r => r.start)), e = Math.max(...rs.map(r => r.start + r.dur));
  console.log(`  ${k.padEnd(12)} ${String(rs.length).padStart(3)} req  ${(enc / 1024).toFixed(0).padStart(5)} KiB wire (${(dec / 1024).toFixed(0)} KiB)  +${ms(s)} → +${ms(e)} ms`);
}
console.log(JSON.stringify({ origin, route, runs, first_row_ms: firsts.map(x => x == null ? null : Math.round(x)), median_ms: median(firsts) == null ? null : Math.round(median(firsts)), ttfb_ms: ttfbs.map(Math.round), socket_open_ms: sockets }));
