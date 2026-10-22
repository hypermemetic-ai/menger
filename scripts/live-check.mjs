#!/usr/bin/env node
// APP-110: the live test app in a ticket (scripts/live-check.sh). Against a
// throwaway tracker, a "test" server it frames, and a bare one.
//
//   node scripts/live-check.mjs TRACKER_SHELL TEST_SHELL BARE_SHELL TRACKER_SESSION TEST_SESSION BARE_SESSION WIDTH THEME [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
import { sessionCookies } from "./throwaway-login.mjs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [aPort, tPort, bPort, aSess, tSess, bSess, widthArg, scheme, shots] = process.argv.slice(2);
const width = Number(widthArg);
const phone = width === 390;
const A = `http://127.0.0.1:${aPort}`, T = `http://127.0.0.1:${tPort}`, B = `http://127.0.0.1:${bPort}`;
const token = (f) => fs.readFileSync(f, "utf8").trim();
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const tag = `${width}-${scheme}`;

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width, height: phone ? 844 : 900 }, colorScheme: scheme, hasTouch: phone });
await ctx.addCookies([...sessionCookies(A, token(aSess)), ...sessionCookies(T, token(tSess)), ...sessionCookies(B, token(bSess))]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));

// -- the headers: the bare server refuses to be framed; test allows the tracker only; the tracker frames test only.
const head = async (origin, path) => {
  const r = await ctx.request.get(`${origin}${path}`, { maxRedirects: 0 });
  const h = r.headers();
  return { csp: h["content-security-policy"] || "", xfo: h["x-frame-options"] };
};
const bare = await head(B, "/app/");
ok(bare.csp.includes("frame-ancestors 'none'") && bare.xfo === "DENY", `${tag}: an unconfigured server refuses to be framed (${bare.csp.match(/frame-ancestors[^;]*/)}, ${bare.xfo})`);
ok(/frame-src 'self';/.test(bare.csp), `${tag}: and frames only itself`);
const test = await head(T, "/app/");
ok(test.csp.includes(`frame-ancestors ${A};`) || test.csp.endsWith(`frame-ancestors ${A}`), `${tag}: test names the tracker's origin as its one ancestor (${test.csp.match(/frame-ancestors[^;]*/)})`);
ok(test.xfo === undefined, `${tag}: and sends no X-Frame-Options: DENY beside it (${test.xfo})`);
const trk = await head(A, "/app/");
ok(trk.csp.includes(`frame-src 'self' ${T}`) && trk.csp.includes("frame-ancestors 'none'"), `${tag}: the tracker frames test and is framed by no one`);
const login = await head(T, "/login");
ok(login.csp.includes("frame-ancestors 'none'") && login.xfo === "DENY", `${tag}: test's sign-in stays unframeable`);
// A real browser's verdict: an iframe of the bare server on a blank page is refused.
await page.goto("about:blank");
const refused = await page.evaluate((src) => new Promise((resolve) => {
  const f = document.createElement("iframe");
  f.src = src;
  f.onload = () => { try { resolve(f.contentDocument ? "loaded" : "blocked"); } catch { resolve("blocked"); } };
  document.body.appendChild(f);
  setTimeout(() => resolve("timeout"), 8000);
}), `${B}/app/`);
ok(refused === "blocked", `${tag}: a browser will not draw the bare server in a frame (${refused})`);

// -- the ticket on the tracker.
await page.goto(`${A}/app/#/ticket/KILN-2`);
const box = (n) => page.locator(`.mmock[data-mock="${n}"]`);
try {
  await page.waitForSelector('.mmock[data-mock="2"] iframe.live-frame', { timeout: 30000, state: "attached" });
} catch {
  if (shots) await page.screenshot({ path: `${shots}/${tag}-none.png` });
  console.log(`FAIL ${tag}: no live view drew (${errs.join(" | ")})`);
  process.exit(1);
}
await page.waitForTimeout(1500);
const srcs = async (n) => box(n).locator("iframe.live-frame").evaluateAll((l) => l.map((f) => f.getAttribute("src")));
const s2 = await srcs(2);
ok(s2.length === 2 && s2.every((s) => s.startsWith(`${T}/`) && s.includes("#/components")), `${tag}: the live block draws one iframe per device (390, 1440) of test's route: ${JSON.stringify(s2)}`);
ok((await box(1).locator("iframe.live-frame").count()) === 0 && (await box(1).locator("iframe").count()) >= 1, `${tag}: the mockup still draws its own frame`);
const s3 = await srcs(3);
ok(s3.length === 1 && s3[0].includes("#/board"), `${tag}: the element form draws one at its width: ${JSON.stringify(s3)}`);
ok((await box(4).locator("iframe").count()) === 0 && /never drawn/.test(await box(4).locator(".mmock-state").innerText()), `${tag}: env=prod is refused in words, nothing framed`);

// The route loads in the frame, signed in on test with its own session.
const shown = (n) => box(n).locator(".device:not(.hidden) iframe.live-frame").first();
const content = async (n) => (await shown(n).elementHandle()).contentFrame();
let fr = await content(2);
let loaded = false;
try { await fr.waitForSelector("#components", { timeout: 30000 }); loaded = true; } catch {}
ok(loaded, `${tag}: the route loads in the frame, signed in as the reader on test (${fr.url()})`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-live.png`, fullPage: false });

// -- nothing overflows.
const fit = await page.evaluate(() => {
  const out = { doc: [document.scrollingElement.scrollWidth, innerWidth], bad: [] };
  document.querySelectorAll('.mmock[data-mock="2"], .mmock[data-mock="3"]').forEach((b) => {
    const br = b.getBoundingClientRect();
    b.querySelectorAll(".device:not(.hidden)").forEach((d) => {
      const s = d.querySelector(".device-screen").getBoundingClientRect();
      const f = d.querySelector(".live-frame").getBoundingClientRect();
      if (s.right > br.right + 1 || s.left < br.left - 1 || s.right > innerWidth + 1) out.bad.push(`screen ${s.left}..${s.right} in ${br.left}..${br.right}`);
      if (f.width < 1 || f.height < 1) out.bad.push("empty frame");
    });
  });
  return out;
});
ok(fit.doc[0] <= fit.doc[1], `${tag}: no horizontal scroll (${fit.doc[0]} <= ${fit.doc[1]})`);
ok(fit.bad.length === 0, `${tag}: every live screen sits inside its box${fit.bad.length ? ": " + fit.bad.join("; ") : ""}`);

// -- the bar's controls. On a phone the bar folds into ⋯.
const press = async (n, sel) => {
  const el = box(n).locator(sel).first();
  await box(n).locator(".framed-bar").first().evaluate((e) => e.scrollIntoView({ block: "center" }));
  if (!(await el.isVisible())) {
    const more = box(n).locator(".framed-more").first();
    if (await more.count()) await more.click();
  }
  await el.click();
};
const closeMore = async (n) => {
  const scrim = box(n).locator(".framed-scrim").first();
  if ((await scrim.count()) && (await scrim.isVisible())) await scrim.evaluate((e) => e.click());
};

// release line
if (phone) {
  await box(2).locator(".framed-bar").first().evaluate((e) => e.scrollIntoView({ block: "center" }));
  await box(2).locator(".framed-more").first().click();
}
const rel = await box(2).locator(".live-release").first().innerText();
ok(/test runs 20261022T0830Z-abc1234/.test(rel), `${tag}: the bar says which release test runs: "${rel}"`);
const open = await box(2).locator("a.live-open").first().getAttribute("href");
ok(open && open.startsWith(`${T}/`) && open.includes("#/components"), `${tag}: Open in a tab points at the route on test`);
await closeMore(2);

// Reload
const before = (await srcs(2))[0];
await press(2, ".live-reload");
await page.waitForTimeout(500);
const after = (await srcs(2))[0];
ok(before !== after && after.includes("reload=1"), `${tag}: Reload loads the frame again (${before} → ${after})`);
await closeMore(2);

// the widths
const picker = box(2).locator(".framed-bar button.framed-btn", { hasText: /^1440$/ }).first();
if (await picker.count()) {
  await picker.click();
  await page.waitForTimeout(300);
  const vis = await box(2).locator(".device:not(.hidden)").evaluateAll((d) => d.map((x) => x.getAttribute("data-w")));
  ok(vis.includes("1440"), `${tag}: the widths picker shows the 1440 device (${vis})`);
  const f1440 = await box(2).locator('.device[data-w="1440"] iframe.live-frame').first().elementHandle();
  const fr1440 = await f1440.contentFrame();
  let l2 = false;
  try { await fr1440.waitForSelector("#components", { timeout: 30000 }); l2 = true; } catch {}
  ok(l2, `${tag}: the 1440 device loads the route too`);
  const g = await box(2).locator('.device[data-w="1440"] .device-screen').evaluate((e) => { const r = e.getBoundingClientRect(); return [r.left, r.right, innerWidth]; });
  ok(g[1] <= g[2] + 1 && g[0] >= -1, `${tag}: and sits inside the screen (${g})`);
} else {
  ok(false, `${tag}: a widths picker`);
}

// Light / Dark follows the box's toggle: the frame is asked for the theme and paints it.
await press(2, '.framed-btn[aria-pressed]:text-is("Dark")');
await page.waitForTimeout(800);
const dsrc = (await srcs(2))[0];
ok(dsrc.includes("theme=dark"), `${tag}: Dark asks the frame for theme=dark (${dsrc})`);
fr = await content(2);
let painted = null;
for (let i = 0; i < 40 && painted !== "dark"; i++) { painted = await fr.evaluate(() => document.documentElement.getAttribute("data-theme")).catch(() => null); if (painted !== "dark") await page.waitForTimeout(300); }
ok(painted === "dark", `${tag}: and the app inside paints dark (data-theme=${painted})`);
await press(2, '.framed-btn[aria-pressed]:text-is("Light")');
await page.waitForTimeout(800);
fr = await content(2);
painted = null;
for (let i = 0; i < 40 && painted !== "light"; i++) { painted = await fr.evaluate(() => document.documentElement.getAttribute("data-theme")).catch(() => null); if (painted !== "light") await page.waitForTimeout(300); }
ok(painted === "light", `${tag}: Light paints it light again (data-theme=${painted})`);
await closeMore(2);
if (shots) await page.screenshot({ path: `${shots}/${tag}-controls.png` });

// Full screen
await press(2, ".framed-full");
await page.waitForTimeout(500);
const full = await box(2).locator(".framed.is-full").count();
ok(full === 1, `${tag}: Full screen opens the live view`);
if (full) {
  const fsrc = await page.evaluate(() => { const f = document.querySelector(".framed.is-full .device:not(.hidden) iframe.live-frame"); const r = f && f.getBoundingClientRect(); return r ? [r.width, r.height] : null; });
  ok(fsrc && fsrc[0] > 100 && fsrc[1] > 100 && (phone || fsrc[0] <= width + 1), `${tag}: full screen, the frame fills a screen (${fsrc})`);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  if (await box(2).locator(".framed.is-full").count()) await box(2).locator(".framed-full").first().click().catch(() => {});
  await page.waitForTimeout(300);
}
await page.evaluate(() => { const m = document.querySelector(".framed-scrim"); if (m && !m.hidden) m.click(); });

// Compare: the mockup (before) and the live view (after) at one device and theme.
await press(1, ".framed-compare");
await closeMore(1);
await press(2, ".framed-compare");
await closeMore(2);
await page.waitForSelector(".cmp-pill:not([hidden]) .cmp-open", { timeout: 10000 });
await page.locator(".cmp-pill .cmp-open").click();
try { await page.waitForSelector(".cmp-view .cmp-col", { timeout: 10000 }); } catch {}
await page.waitForTimeout(1500);
const cols = await page.evaluate(() => [...document.querySelectorAll(".cmp-view .cmp-col")].map((c) => ({ key: c.getAttribute("data-compare-item"), live: c.querySelectorAll("iframe.live-frame").length, other: c.querySelectorAll("iframe:not(.live-frame)").length, hidden: c.classList.contains("hidden") })));
const visible = cols.filter((c) => !c.hidden);
ok(cols.length === 2 && cols.some((c) => c.live === 1) && cols.some((c) => c.other >= 1 && c.live === 0), `${tag}: Compare sets the mockup and the live view side by side: ${JSON.stringify(cols)}`);
ok(visible.length >= 1, `${tag}: and a column is on screen`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-compare.png` });
await page.keyboard.press("Escape");

// -- a server with no test app configured says so.
await page.goto(`${B}/app/#/ticket/KILN-2`);
try { await page.waitForSelector('.mmock[data-mock="2"] .mmock-state:not([hidden])', { timeout: 30000 }); } catch {}
const none = await page.locator('.mmock[data-mock="2"] .mmock-state').innerText().catch(() => "");
ok(/no test app is configured/.test(none) && (await page.locator('.mmock[data-mock="2"] iframe.live-frame').count()) === 0, `${tag}: with no --live-origin the block says so and frames nothing ("${none}")`);

ok(errs.length === 0, `${tag}: no page errors${errs.length ? ": " + errs.slice(0, 3).join(" | ") : ""}`);
await browser.close();
process.exit(fails.length ? 1 : 0);
