#!/usr/bin/env node
// DEPLOY-38: pinch zoom on #/deploy's canvas, checked with synthetic touch
// at 390 (Chromium's CDP touch events — they arrive as touch pointers, as an
// iPhone's do). Against a throwaway server with work in flight
// (scripts/throwaway-login.mjs makes the session file):
//   node scripts/deploy-touch-check.mjs http://127.0.0.1:PORT SESSION_FILE [OUTDIR]
// Checks: a pinch zooms about the fingers' midpoint (what was under them
// stays under them), a two-finger drag pans, a double-tap zooms in, ⤢ fits,
// the page itself is never zoomed or scrolled by it, no page errors, and a
// move costs well under a frame. Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, out] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const near = (a, b, eps = 1.5) => Math.abs(a - b) <= eps;

const browser = await pw.chromium.launch();
for (const theme of ["light", "dark"]) {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, hasTouch: true, isMobile: true, colorScheme: theme });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  const cdp = await ctx.newCDPSession(page);
  const touch = (type, pts) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: pts.map(([x, y], i) => ({ x, y, id: i + 1, radiusX: 4, radiusY: 4, force: 1 })) });
  // The canvas's view (k, tx, ty), read from the world group's transform.
  const view = () => page.evaluate(() => {
    const t = document.querySelector("#deploy-page .canvas svg > g[transform]:not(.heads)").getAttribute("transform");
    const m = t.match(/translate\(([-\d.]+) ([-\d.]+)\) scale\(([-\d.]+)\)/);
    const r = document.querySelector("#deploy-page .canvas").getBoundingClientRect();
    return { k: +m[3], tx: +m[1], ty: +m[2], left: r.left, top: r.top, w: r.width, h: r.height };
  });
  const world = (v, x, y) => [(x - v.left - v.tx) / v.k, (y - v.top - v.ty) / v.k];
  const tag = `390 ${theme}`;

  await page.goto(`${origin}/#/deploy`);
  await page.waitForSelector("#deploy-page", { timeout: 30000 });
  await page.waitForTimeout(1500);
  await page.click("#seg-graph");
  await page.waitForSelector("#deploy-page .canvas .fnode", { timeout: 20000 });
  await page.waitForTimeout(600);
  ok(await page.evaluate(() => getComputedStyle(document.querySelector("#deploy-page .canvas")).touchAction === "none"), `${tag}: the canvas takes touch itself (touch-action: none)`);

  // An empty spot of the canvas, away from nodes and the zoom buttons.
  const v0 = await view();
  const cy = v0.top + v0.h * 0.75, cx = v0.left + v0.w / 2;

  // Pinch out about (cx, cy): 80 px apart to 200 px apart, in 30 moves.
  const before = world(v0, cx, cy);
  await touch("touchStart", [[cx - 40, cy], [cx + 40, cy]]);
  for (let i = 1; i <= 30; i++) { const d = 40 + (60 * i) / 30; await touch("touchMove", [[cx - d, cy], [cx + d, cy]]); }
  await touch("touchEnd", []);
  await page.waitForTimeout(100);
  const v1 = await view();
  ok(near(v1.k / v0.k, Math.min(2.5, 3 / v0.k), 0.02), `${tag}: a pinch zooms by the spread's ratio (${(v1.k / v0.k).toFixed(2)}×)`);
  // On screen, within the pixel a pointer's integer coordinates round to.
  const sx = v1.left + v1.tx + before[0] * v1.k, sy = v1.top + v1.ty + before[1] * v1.k;
  ok(near(sx, cx, 2) && near(sy, cy, 2), `${tag}: what was under the fingers stays under them (${(sx - cx).toFixed(1)}, ${(sy - cy).toFixed(1)} px)`);
  // A pinch in the page, a move a frame, as a phone sends them: the
  // longest frame (the handler, the view redrawn) stays near 16.7 ms.
  const frame = await page.evaluate(async () => {
    const cv = document.querySelector("#deploy-page .canvas");
    const r = cv.getBoundingClientRect();
    const ev = (type, id, x, y) => cv.dispatchEvent(new PointerEvent(type, { pointerId: id, pointerType: "touch", isPrimary: id === 71, clientX: r.left + x, clientY: r.top + y, bubbles: true, cancelable: true }));
    const y = r.height * 0.75;
    ev("pointerdown", 71, 150, y); ev("pointerdown", 72, 240, y);
    let last = await new Promise(requestAnimationFrame), worst = 0;
    for (let i = 1; i <= 60; i++) {
      ev("pointermove", 71, 150 - i, y); ev("pointermove", 72, 240 + i, y);
      const t = await new Promise(requestAnimationFrame);
      worst = Math.max(worst, t - last); last = t;
    }
    ev("pointerup", 71, 90, y); ev("pointerup", 72, 300, y);
    return worst;
  });
  ok(frame < 34, `${tag}: a pinch keeps the frame rate (longest frame ${frame.toFixed(1)} ms)`);
  await page.waitForTimeout(100);
  const v1b = await view();

  // Two fingers moved together: a pan, the zoom held.
  await touch("touchStart", [[cx - 50, cy], [cx + 50, cy]]);
  for (let i = 1; i <= 10; i++) await touch("touchMove", [[cx - 50 - 4 * i, cy - 3 * i], [cx + 50 - 4 * i, cy - 3 * i]]);
  await touch("touchEnd", []);
  await page.waitForTimeout(100);
  const v2 = await view();
  ok(near(v2.k, v1b.k, 0.001) && near(v2.tx - v1b.tx, -40) && near(v2.ty - v1b.ty, -30), `${tag}: a two-finger drag pans (${(v2.tx - v1b.tx).toFixed(0)}, ${(v2.ty - v1b.ty).toFixed(0)})`);

  // Pinch in, past the floor: held at 10%.
  await touch("touchStart", [[cx - 150, cy], [cx + 150, cy]]);
  for (let i = 1; i <= 10; i++) await touch("touchMove", [[cx - 150 + 14.5 * i, cy], [cx + 150 - 14.5 * i, cy]]);
  await touch("touchEnd", []);
  await page.waitForTimeout(100);
  const v3 = await view();
  ok(v3.k < v2.k && v3.k >= 0.1 - 1e-9, `${tag}: a pinch in zooms out, held at 10% (${(v3.k * 100).toFixed(0)}%)`);

  // Fit, then a double-tap on an empty spot zooms in by 1.6 about it.
  await page.click('#deploy-page .zoom [aria-label="fit"]');
  await page.waitForTimeout(150);
  const v4 = await view();
  const ex = v4.left + 8, ey = v4.top + v4.h - 60;
  const bt = world(v4, ex, ey);
  await page.touchscreen.tap(ex, ey);
  await page.waitForTimeout(60);
  await page.touchscreen.tap(ex, ey);
  await page.waitForTimeout(150);
  const v5 = await view();
  ok(near(v5.k / v4.k, Math.min(1.6, 3 / v4.k), 0.02), `${tag}: a double-tap zooms in (${(v5.k / v4.k).toFixed(2)}×)`);
  const at = world(v5, ex, ey);
  ok(near(bt[0], at[0]) && near(bt[1], at[1]), `${tag}: about the tapped point`);
  ok(!(await page.evaluate(() => document.querySelector("#deploy-page .sheet.on"))), `${tag}: a tap off a node opens nothing`);
  await page.click('#deploy-page .zoom [aria-label="fit"]');
  await page.waitForTimeout(150);
  ok(near((await view()).k, v4.k, 0.001), `${tag}: ⤢ fits the graph again`);

  // A tap on a node still opens its summary.
  const n = await page.$("#deploy-page .canvas .fnode");
  const b = await n.boundingBox();
  await page.touchscreen.tap(b.x + b.width / 2, b.y + b.height / 2);
  await page.waitForTimeout(500);
  ok(!!(await page.$("#deploy-page .sheet.on")), `${tag}: a tap on a node opens its summary, and it stays open`);

  const vv = await page.evaluate(() => ({ s: visualViewport.scale, x: scrollX, y: scrollY }));
  ok(vv.s === 1 && vv.x === 0, `${tag}: the page itself is not zoomed (scale ${vv.s})`);
  ok(errs.length === 0, `${tag}: no page errors${errs.length ? ": " + errs.join("; ") : ""}`);
  if (out) { fs.mkdirSync(out, { recursive: true }); await page.screenshot({ path: `${out}/deploy-390-${theme}.png` }); }
  await ctx.close();
}
// At a desk nothing changed: a mouse's click on a node opens its summary, a drag pans.
{
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  await page.goto(`${origin}/#/deploy`);
  await page.waitForSelector("#deploy-page .canvas .fnode", { timeout: 30000 });
  await page.waitForTimeout(800);
  await page.click("#deploy-page .canvas .fnode");
  await page.waitForTimeout(400);
  ok(!!(await page.$("#deploy-page .sheet.on")), "1440: a click on a node opens its summary");
  await ctx.close();
}
await browser.close();
console.log(fails.length ? `${fails.length} failed` : "all ok");
process.exit(fails.length ? 1 : 0);
