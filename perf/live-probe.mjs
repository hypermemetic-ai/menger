// FRAME-16: the first row on the wire — cold loads of a deployed menger (app-test by default),
// each in a fresh browser context signed in by a session cookie read from a file (never argv):
//
//   node perf/live-probe.mjs --token-file FILE [--origin https://app-test.menger.sh] [--runs 5] [--path /]
//
// Prints the first row's median, the boot's marks, FRAME-17's table of the steps after the boot
// (below) and the waterfall by kind, then one JSON line. A table is its output, not a pass or a
// fail: nothing here is a test (the hand gate reads it, three runs of five, before and after).
//
// Prints: the first titled board row's median over the runs (navigation start → the first
// `.board-row[data-id] [data-field=title]`), TTFB, when the socket opened, and the waterfall by
// kind (requests, bytes on the wire and decoded, from → to) of the last run — the shape FRAME-16
// changes. A warm-up load runs first and is not counted. Playwright comes from perf/node_modules.
//
// The hand gate's marks, on the page's own clock (an init script installed before anything of
// the page runs): when the page made its socket and when it really opened (its `open` event,
// not Playwright's "websocket" event), and when WebAssembly.instantiate(Streaming) resolved —
// the median of each, and in how many runs the socket opened before the wasm was instantiated;
// the JSON carries every run's marks and the last run's requests counted by kind.
//
// FRAME-17: the first row after the boot, as a table (FRAME-17's), the median and each run on the
// page's clock — the boot done (the socket open and the wasm instantiated), the first call and
// `tickets.list` sent (every frame the page sends, stamped), `tickets.list` answered (its first
// data and its `done`, matched to the call by its subscription), the nav drawn, the first titled
// row — then every mark the host sets itself (`performance.mark`, `plexus-ui:*`: components parsed,
// each section and the layout mounted, each component's handlers compiled, the first call, answer
// and row; CONTRACT.md's host chapter), or a line saying the engine sets none.
//
// PERF-15: the bytes per call — every frame the page's sockets received, from CDP's
// Network.webSocketFrameReceived (its payload's UTF-8 length, as PLEXUS-10 measured), matched to its
// call by the request id (the ack) and the subscription (each item): per call its frames and bytes,
// the median over the runs, and the whole of the board's calls. Today `tickets.list` is 202 frames
// (a frame per row, the page, done); after PERF-15 three (one `rows`, the page, done), and the stream
// row (first data → done) ~0.
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

const MARKS = `(() => {
  const log = (window.__probe = {});
  const frames = (window.__frames = { sent: [], recv: [] });
  const now = () => performance.now();
  const W = window.WebSocket;
  window.WebSocket = class extends W {
    constructor(...a) {
      super(...a);
      log.socket_made ??= now();
      this.addEventListener("open", () => { log.socket_open ??= now(); });
      this.addEventListener("message", (e) => {
        const t = now();
        try {
          const m = JSON.parse(e.data);
          frames.recv.push([t, m.params?.subscription ?? null, m.params?.result?.type ?? null, m.id ?? null, typeof m.result === "number" ? m.result : null]);
        } catch {}
      });
    }
  };
  const send = W.prototype.send;
  W.prototype.send = function (d) {
    try { const m = JSON.parse(d); frames.sent.push([now(), m.params?.method ?? m.method, m.id ?? null]); } catch {}
    return send.call(this, d);
  };
  new MutationObserver(() => {
    const t = now();
    if (log.nav_drawn == null && document.querySelector("[data-nav-group]")) log.nav_drawn = t;
    if (log.first_row == null) for (const el of document.querySelectorAll(".board-row[data-id] [data-field=title]")) if (el.textContent.trim()) { log.first_row = t; break; }
  }).observe(document, { childList: true, subtree: true, characterData: true });
  for (const k of ["instantiateStreaming", "instantiate"]) {
    const f = WebAssembly[k];
    WebAssembly[k] = function (...a) { return f.apply(this, a).then((r) => { log.wasm_instantiated ??= now(); return r; }); };
  }
})();`;

const browser = await chromium.launch();
const firsts = [], ttfbs = [], sockets = [], marks = [], calls = [];
// PERF-15: each call's frames and bytes (the ack's included), keyed by its method — a method asked
// more than once (the three watches) summed under its name.
function perCall(wire) {
  const len = (s) => Buffer.byteLength(s, "utf8");
  const byId = new Map(), bySub = new Map(), out = {};
  for (const [sock, d] of wire.sent) { try { const m = JSON.parse(d); if (m.id != null) byId.set(`${sock}/${m.id}`, m.params?.method ?? m.method); } catch {} }
  const add = (method, n) => { const c = (out[method] ||= { frames: 0, bytes: 0 }); c.frames++; c.bytes += n; };
  for (const [sock, d] of wire.recv) {
    let m; try { m = JSON.parse(d); } catch { continue; }
    if (m.id != null && byId.has(`${sock}/${m.id}`)) { const method = byId.get(`${sock}/${m.id}`); if (m.result != null) bySub.set(`${sock}/${m.result}`, method); add(method, len(d)); continue; }
    add(bySub.get(`${sock}/${m.params?.subscription}`) ?? "(other)", len(d));
  }
  return out;
}
let last = null;
for (let i = 0; i <= runs; i++) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await ctx.addCookies([{ name: cookieName, value: token, url: origin, secure: origin.startsWith("https://"), httpOnly: true, sameSite: "Lax" }]);
  const page = await ctx.newPage();
  await page.addInitScript(MARKS);
  const opened = []; page.on("websocket", () => opened.push(Date.now()));
  // PERF-15: what crossed, per frame, off the page's clock (CDP's network view).
  const wire = { sent: [], recv: [] };
  const cdp = await ctx.newCDPSession(page);
  await cdp.send("Network.enable");
  cdp.on("Network.webSocketFrameSent", ({ requestId, response }) => wire.sent.push([requestId, response.payloadData]));
  cdp.on("Network.webSocketFrameReceived", ({ requestId, response }) => wire.recv.push([requestId, response.payloadData]));
  const t0 = Date.now();
  await page.goto(origin + route, { waitUntil: "commit" });
  const ok = await page.waitForSelector(".board-row[data-id] [data-field=title]", { timeout: 30000 }).then(() => true).catch(() => false);
  const first = ok ? await page.evaluate(() => performance.now()) : null;
  const nav = await page.evaluate(() => { const n = performance.getEntriesByType("navigation")[0]; return { ttfb: n.responseStart, proto: n.nextHopProtocol }; });
  const res = await page.evaluate(() => performance.getEntriesByType("resource").map(r => ({ name: r.name.replace(location.origin, ""), start: r.startTime, dur: r.duration, enc: r.encodedBodySize, dec: r.decodedBodySize })));
  const mark = await page.evaluate(() => window.__probe ?? {});
  // FRAME-17: the board's list, matched by its request id to its subscription; the host's own marks.
  const frames = await page.evaluate(() => window.__frames ?? { sent: [], recv: [] });
  const ask = frames.sent.find((f) => f[1] === "tickets.list");
  const sub = ask && frames.recv.find((r) => r[3] === ask[2] && r[4] != null)?.[4];
  const of_list = (type) => (sub == null ? null : frames.recv.find((r) => r[1] === sub && r[2] === type)?.[0] ?? null);
  const ack = ask && frames.recv.find((r) => r[3] === ask[2] && r[4] != null)?.[0];
  // The first `live` snapshot: not the host's own (ui.*), not a changes stream.
  const snap = frames.sent.find((f) => f[1] && !f[1].startsWith("ui.") && !f[1].endsWith(".watch") && !f[1].endsWith("call_unsub"));
  Object.assign(mark, { first_call: snap?.[0] ?? null, list_sent: ask?.[0] ?? null, list_acked: ack ?? null, list_first_data: of_list("data"), list_done: of_list("done") });
  for (const m of await page.evaluate(() => performance.getEntriesByType("mark").map((m) => [m.name, m.startTime]))) if (m[0].startsWith("plexus-ui:")) mark[m[0]] ??= m[1];
  const bytes = perCall(wire);
  if (i > 0) { firsts.push(first); ttfbs.push(nav.ttfb); sockets.push(opened.length ? opened[0] - t0 : null); marks.push(mark); calls.push(bytes); last = { res, nav, first }; }
  await ctx.close();
}
await browser.close();
const ms = (x) => (x == null ? "—" : `${Math.round(x)}`);
const median = (a) => { const b = a.filter(x => x != null).sort((x, y) => x - y); return b.length ? b[Math.floor(b.length / 2)] : null; };
console.log(`${origin}${route}: first titled row median ${ms(median(firsts))} ms (${firsts.map(ms).join(", ")}); TTFB median ${ms(median(ttfbs))} ms; ${last.nav.proto}`);
const of = (k) => marks.map((m) => m[k] ?? null);
const before = marks.filter((m) => m.socket_open != null && m.wasm_instantiated != null && m.socket_open < m.wasm_instantiated).length;
console.log(`  socket made +${ms(median(of("socket_made")))} ms, open +${ms(median(of("socket_open")))} ms; wasm instantiated +${ms(median(of("wasm_instantiated")))} ms; the socket open first in ${before} of ${marks.length} runs`);
// FRAME-17: the table — each step's median and each run's, on the page's clock.
const steps = [
  ["the boot done (socket open, wasm instantiated)", (m) => (m.socket_open == null || m.wasm_instantiated == null ? null : Math.max(m.socket_open, m.wasm_instantiated))],
  ["the first `live` snapshot sent", (m) => m.first_call],
  ["`tickets.list` sent", (m) => m.list_sent],
  ["`tickets.list` acknowledged", (m) => m.list_acked],
  ["`tickets.list` answered (its first data)", (m) => m.list_first_data],
  ["`tickets.list` answered (its done)", (m) => m.list_done],
  ["the nav drawn", (m) => m.nav_drawn],
  ["the first titled row", (m) => m.first_row],
];
// The gaps the acceptance and the owner read: each a difference, its median and every run's.
const gap = (a, b) => (m) => (m[a] == null || m[b] == null ? null : m[a] - m[b]);
const gaps = [
  ["`tickets.list` sent − the socket open (acceptance: ≤ 20 ms)", gap("list_sent", "socket_open")],
  ["the ack wait: `tickets.list` sent → acknowledged (PLEXUS-10)", gap("list_acked", "list_sent")],
  ["acknowledged → its first data", gap("list_first_data", "list_acked")],
  ["the stream: first data → done (PERF-15: one `rows` frame, ~0 after)", gap("list_done", "list_first_data")],
  ["the board's draw: `drawn:tickets.list` − `answered:tickets.list` (acceptance: ≤ 20 ms for 200 rows)", gap("plexus-ui:drawn:tickets.list", "plexus-ui:answered:tickets.list")],
  ["the first draw: `plexus-ui:first-row` − `plexus-ui:first-answer`", gap("plexus-ui:first-row", "plexus-ui:first-answer")],
  ["done → the first titled row", gap("first_row", "list_done")],
];
const hostMarks = [...new Set(marks.flatMap((m) => Object.keys(m).filter((k) => k.startsWith("plexus-ui:"))))].sort((a, b) => median(of(a)) - median(of(b)));
for (const k of hostMarks) steps.push([`\`${k}\``, (m) => m[k]]);
console.log("| mark | median | runs |\n| -- | -- | -- |");
for (const [what, f] of steps) { const xs = marks.map((m) => f(m) ?? null); console.log(`| ${what} | +${ms(median(xs))} ms | ${xs.map(ms).join(", ")} |`); }
if (!hostMarks.length) console.log("  (the engine sets no plexus-ui:* marks: a release before FRAME-17)");
// PERF-15: the bytes per call (the ack included), the median over the runs, biggest first.
const methods = [...new Set(calls.flatMap((c) => Object.keys(c)))].sort((a, b) => median(calls.map((c) => c[b]?.bytes ?? 0)) - median(calls.map((c) => c[a]?.bytes ?? 0)));
console.log("| call | frames | bytes (median) | runs |\n| -- | -- | -- | -- |");
for (const k of methods) console.log(`| \`${k}\` | ${median(calls.map((c) => c[k]?.frames ?? 0))} | ${median(calls.map((c) => c[k]?.bytes ?? 0))} | ${calls.map((c) => c[k]?.bytes ?? 0).join(", ")} |`);
const totals = calls.map((c) => Object.values(c).reduce((a, x) => ({ frames: a.frames + x.frames, bytes: a.bytes + x.bytes }), { frames: 0, bytes: 0 }));
console.log(`| every call | ${median(totals.map((t) => t.frames))} | ${median(totals.map((t) => t.bytes))} | ${totals.map((t) => t.bytes).join(", ")} |`);
console.log("| gap | median | runs |\n| -- | -- | -- |");
for (const [what, f] of gaps) { const xs = marks.map((m) => f(m)); console.log(`| ${what} | ${ms(median(xs))} ms | ${xs.map(ms).join(", ")} |`); }
const kind = (n) => n.includes("engine.wasm") ? "engine.wasm" : n.includes("engine.js") ? "engine.js" : n.includes("host.js") ? "host.js" : n.includes("/components") ? "components" : n.includes("manifest") ? "manifest" : "other";
const by = {}; for (const r of last.res) (by[kind(r.name)] ||= []).push(r);
const requests = Object.fromEntries(Object.entries(by).map(([k, rs]) => [k, rs.length]));
for (const [k, rs] of Object.entries(by)) {
  const enc = rs.reduce((a, r) => a + (r.enc || 0), 0), dec = rs.reduce((a, r) => a + (r.dec || 0), 0);
  const s = Math.min(...rs.map(r => r.start)), e = Math.max(...rs.map(r => r.start + r.dur));
  console.log(`  ${k.padEnd(12)} ${String(rs.length).padStart(3)} req  ${(enc / 1024).toFixed(0).padStart(5)} KiB wire (${(dec / 1024).toFixed(0)} KiB)  +${ms(s)} → +${ms(e)} ms`);
}
console.log(JSON.stringify({ origin, route, runs, first_row_ms: firsts.map(x => x == null ? null : Math.round(x)), median_ms: median(firsts) == null ? null : Math.round(median(firsts)), ttfb_ms: ttfbs.map(Math.round), socket_open_ms: sockets, marks: marks.map((m) => Object.fromEntries(Object.entries(m).map(([k, v]) => [k, v == null ? null : Math.round(v)]))), socket_open_before_wasm: before, requests, calls }));
