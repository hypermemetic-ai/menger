#!/usr/bin/env node
// APP-92: what a phone sees from the moment menger is opened — measured.
//
// Against a throwaway server signed in with --identity memory (its social
// buttons approve at once) and a key session (scripts/throwaway-login.mjs):
//
//   node scripts/first-paint.mjs OUTDIR http://127.0.0.1:PORT SESSION_FILE [ROUTE]
//
// The shell itself compresses nothing; app.menger.sh's edge does (zstd). To
// measure what a phone gets, put a compressing proxy in front and name the
// shell's own port for the cookie and the sign-in (which redirects to the
// public URL): FIRST_PAINT_COOKIE_PORT=<shell port>,
// FIRST_PAINT_LOGIN=http://127.0.0.1:<shell port>.
//
// For light and dark, on a fast link and on slow 3G (Chromium's throttling:
// 400 kbps, 400 ms RTT), at 390 × 844:
//
// - **cold**: the app opened signed in with nothing cached;
// - **reload**: the same page reloaded (whatever it cached);
// - the time to the first paint, to the first frame that is not empty (one
//   colour) and to the app's first view (`#boot` gone, the route drawn);
// - every empty frame after something was drawn (a flash);
// - **login**: the sign-in page — whether the app is cached behind it after
//   a few seconds, and what a double press of a button sends
//   (FIRST_PAINT_ONLY=login runs only this).
//
// Frames are the screencast's (CDP), decoded here (PNG, zlib); a frame is
// empty when 99.5% of its pixels are one colour. Prints one table and a JSON
// line per run; OUTDIR gets the first non-empty frame of each run.
import { createRequire } from "node:module";
import fs from "node:fs";
import zlib from "node:zlib";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [out, origin, tokenFile, route = "#/chats/1"] = process.argv.slice(2);
fs.mkdirSync(out, { recursive: true });
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const loginOrigin = process.env.FIRST_PAINT_LOGIN || origin;
const cookie = { name: u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${process.env.FIRST_PAINT_COOKIE_PORT || u.port}`, secure: u.protocol === "https:", value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const NETS = {
  fast: null,
  slow3g: { offline: false, latency: 400, downloadThroughput: (400 * 1024) / 8, uploadThroughput: (400 * 1024) / 8 },
};
const CAP_MS = Number(process.env.FIRST_PAINT_CAP_MS || 30000);

// ── PNG: enough to read the screencast's RGBA frames ──
function decodePng(buf) {
  let i = 8, w = 0, h = 0, ct = 0;
  const idat = [];
  while (i < buf.length) {
    const len = buf.readUInt32BE(i), type = buf.toString("ascii", i + 4, i + 8), data = buf.subarray(i + 8, i + 8 + len);
    if (type === "IHDR") { w = data.readUInt32BE(0); h = data.readUInt32BE(4); ct = data[9]; }
    else if (type === "IDAT") idat.push(data);
    i += 12 + len;
  }
  const bpp = ct === 6 ? 4 : 3;
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const stride = w * bpp, px = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)], row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? px[y * stride + x - bpp] : 0, b = y ? px[(y - 1) * stride + x] : 0, c = x >= bpp && y ? px[(y - 1) * stride + x - bpp] : 0;
      let v = row[x];
      if (f === 1) v += a; else if (f === 2) v += b; else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) { const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      px[y * stride + x] = v & 255;
    }
  }
  return { w, h, bpp, px };
}
// The frame's main colour and the share of pixels in it.
function look(png) {
  const { w, h, bpp, px } = decodePng(png);
  const counts = new Map();
  let n = 0;
  for (let y = 0; y < h; y += 2) for (let x = 0; x < w; x += 2) {
    const o = (y * w + x) * bpp, k = (px[o] << 16) | (px[o + 1] << 8) | px[o + 2];
    counts.set(k, (counts.get(k) || 0) + 1);
    n++;
  }
  let top = 0, best = 0;
  for (const [k, c] of counts) if (c > best) { best = c; top = k; }
  return { share: best / n, colour: `#${top.toString(16).padStart(6, "0")}` };
}

const browser = await pw.chromium.launch();
const rows = [];

async function run({ scheme, net, mode }) {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 1, isMobile: true, hasTouch: true, colorScheme: scheme });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  if (mode === "reload") {
    // A first visit, unthrottled, to warm whatever the app caches.
    await page.goto(`${origin}/${route}`);
    await page.waitForFunction(() => !document.getElementById("boot") && document.querySelector("main, #chatfull, #app, body > *"), null, { timeout: 60000 }).catch(() => {});
    await page.waitForTimeout(4000);
  }
  if (NETS[net]) await cdp.send("Network.emulateNetworkConditions", NETS[net]);
  const frames = [];
  cdp.on("Page.screencastFrame", async (f) => {
    frames.push({ at: Date.now(), data: f.data });
    cdp.send("Page.screencastFrameAck", { sessionId: f.sessionId }).catch(() => {});
  });
  await cdp.send("Page.startScreencast", { format: "png", everyNthFrame: 1 });
  const t0 = Date.now();
  const nav = mode === "reload" ? page.reload({ waitUntil: "commit", timeout: CAP_MS }) : page.goto(`${origin}/${route}`, { waitUntil: "commit", timeout: CAP_MS });
  await nav.catch(() => {});
  // The app's first view: the boot sheet gone and the route's content in.
  const ready = await page
    .waitForFunction(() => !document.getElementById("boot") && !document.querySelector("[data-pending], #checking") && document.querySelector("#chatfull .trow, #board tr, main"), null, { timeout: CAP_MS, polling: 50 })
    .then(() => Date.now() - t0)
    .catch(() => null);
  await page.waitForTimeout(300);
  await cdp.send("Page.stopScreencast").catch(() => {});
  const paints = await page.evaluate(() => Object.fromEntries(performance.getEntriesByType("paint").map((p) => [p.name, Math.round(p.startTime)]))).catch(() => ({}));
  let firstContent = null, emptyAfter = 0, emptyColours = new Set(), saved = false;
  for (const f of frames) {
    const l = look(Buffer.from(f.data, "base64"));
    const empty = l.share > 0.995;
    if (!empty && firstContent == null) {
      firstContent = f.at - t0;
      if (!saved) { fs.writeFileSync(`${out}/${mode}-${scheme}-${net}-first.png`, Buffer.from(f.data, "base64")); saved = true; }
    }
    if (empty) {
      emptyColours.add(l.colour);
      if (firstContent != null) emptyAfter++;
    }
  }
  const row = { mode, scheme, net, firstPaint: paints["first-paint"] ?? null, firstContent, ready, frames: frames.length, emptyAfter, emptyColours: [...emptyColours] };
  rows.push(row);
  console.log(JSON.stringify(row));
  await ctx.close();
}

// The sign-in page: is the app cached behind it, and what does a double
// press of a way in send?
async function login({ scheme }) {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, colorScheme: scheme });
  const page = await ctx.newPage();
  const posts = [];
  page.on("request", (r) => { if (r.method() === "POST" && new URL(r.url()).pathname.startsWith("/login")) posts.push(new URL(r.url()).pathname); });
  await page.goto(`${loginOrigin}/login?next=${encodeURIComponent("/" + route)}`);
  await page.waitForTimeout(6000);
  const cached = await page.evaluate(async () => {
    if (!("caches" in self)) return { caches: [] };
    const names = await caches.keys();
    const keys = [];
    for (const n of names) for (const r of await (await caches.open(n)).keys()) keys.push(new URL(r.url).pathname);
    return { sw: !!navigator.serviceWorker?.controller || !!(await navigator.serviceWorker?.getRegistration()), wasm: keys.some((k) => k.endsWith(".wasm")), js: keys.some((k) => /menger-app-.*\.js$/.test(k)), n: keys.length };
  });
  // Hold each sign-in POST 2 s, as a slow provider does, so the second
  // press lands while the first is still out.
  await page.route(/\/login\//, async (r) => { if (r.request().method() === "POST") await new Promise((ok) => setTimeout(ok, 2000)); await r.continue(); });
  const social = page.locator('form[action="/login/social"] button');
  const btn = (await social.count()) ? social.nth(Math.min(1, (await social.count()) - 1)) : page.locator("form button, a.alt").first();
  const aborted = [];
  page.on("requestfailed", (r) => { if (r.method() === "POST") aborted.push(r.failure()?.errorText); });
  // Pressed by the mouse, not the locator: a navigation is pending after the
  // first press and Playwright's locators wait it out. The frame 300 ms after
  // the first press is kept (the spinner, or not).
  const bb = await btn.boundingBox();
  if (bb) {
    const cdp = await ctx.newCDPSession(page);
    let shot = null;
    cdp.on("Page.screencastFrame", (f) => { shot = f.data; cdp.send("Page.screencastFrameAck", { sessionId: f.sessionId }).catch(() => {}); });
    await cdp.send("Page.startScreencast", { format: "png", everyNthFrame: 1 });
    await page.mouse.click(bb.x + bb.width / 2, bb.y + bb.height / 2);
    await page.waitForTimeout(300);
    if (shot) fs.writeFileSync(`${out}/login-${scheme}-pressed.png`, Buffer.from(shot, "base64"));
    await page.mouse.click(bb.x + bb.width / 2, bb.y + bb.height / 2);
    await cdp.send("Page.stopScreencast").catch(() => {});
    await page.waitForTimeout(5000);
  }
  const landed = new URL(page.url()).pathname;
  const err = await page.locator(".error").first().textContent({ timeout: 500 }).catch(() => null);
  const row = { mode: "login", scheme, cached, presses: 2, posts: posts.length, aborted: aborted.length, landed, error: err };
  rows.push(row);
  console.log(JSON.stringify(row));
  await ctx.close();
}

for (const scheme of ["light", "dark"]) {
  for (const net of process.env.FIRST_PAINT_ONLY === "login" ? [] : Object.keys(NETS)) {
    await run({ scheme, net, mode: "cold" });
    await run({ scheme, net, mode: "reload" });
  }
  await login({ scheme });
}
await browser.close();
const f = (n) => (n == null ? "—" : `${n}`);
console.log("\nmode    scheme net     first-paint first-content ready  empty-after  empty colours");
for (const r of rows.filter((r) => r.mode !== "login")) console.log(`${r.mode.padEnd(7)} ${r.scheme.padEnd(6)} ${r.net.padEnd(7)} ${f(r.firstPaint).padStart(11)} ${f(r.firstContent).padStart(13)} ${f(r.ready).padStart(6)} ${f(r.emptyAfter).padStart(12)}  ${r.emptyColours.join(",")}`);
for (const r of rows.filter((r) => r.mode === "login")) console.log(`login ${r.scheme}: app cached behind it ${JSON.stringify(r.cached)}; two presses sent ${r.posts} POST(s) (${r.aborted} aborted); landed ${r.landed}${r.error ? `, error "${r.error}"` : ""}`);
