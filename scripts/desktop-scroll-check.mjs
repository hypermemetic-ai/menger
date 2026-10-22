#!/usr/bin/env node
// APP-112: on a desktop (> 820 px) the window never scrolls — #nav and
// #app .content scroll inside themselves, so the document is exactly the
// viewport's height on every page, and a wheel at the end of either column
// moves nothing else. On a phone (≤ 820) the window is the scroller, and it
// still ends where the page does. Against a throwaway
// (scripts/desktop-scroll-check.sh); with SHOTS, a shot of each page with
// the window scrolled as far as it goes and the nav at its end.
//
//   node scripts/desktop-scroll-check.mjs ORIGIN SESSION_FILE WxH THEME LONG_TICKET [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, size, scheme, long, shots] = process.argv.slice(2);
const [width, height] = size.split("x").map(Number);
const phone = width <= 820;
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: scheme, hasTouch: phone });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const tag = `${size}-${scheme}`;

// What makes the document taller than the viewport, by name: an element
// whose box ends below it that no scroller of its own clips.
const culprits = () => page.evaluate(() => {
  const out = [];
  const name = (e) => `${e.tagName.toLowerCase()}${e.id ? "#" + e.id : ""}${typeof e.className === "string" && e.className ? "." + e.className.trim().split(/\s+/).join(".") : ""}`;
  for (const e of document.querySelectorAll("body *")) {
    const b = e.getBoundingClientRect();
    if (!b.height || b.bottom + scrollY <= innerHeight + 1) continue;
    const cs = getComputedStyle(e);
    if (cs.position !== "absolute" && cs.position !== "fixed") continue;
    out.push(`${name(e)} (${cs.position}, ends at ${Math.round(b.bottom + scrollY)}, against ${e.offsetParent ? name(e.offsetParent) : "the page"})`);
  }
  return out.slice(0, 5);
});
const doc = () => page.evaluate(() => ({ sh: document.scrollingElement.scrollHeight, ih: innerHeight, sw: document.scrollingElement.scrollWidth, iw: innerWidth }));

const routes = [["board", "#/board"], ["ticket", `#/ticket/${long}`], ["chats", "#/chats"], ["chat", "#/chats/1"], ["components", "#/components"]];
for (const [label, route] of routes) {
  await page.goto(`${origin}/app/${route}`);
  try {
    await page.waitForFunction(() => /live/.test(document.querySelector("#conn")?.textContent ?? ""), null, { timeout: 30000 });
    await page.waitForFunction(() => !/loading…/.test(document.querySelector("main")?.textContent ?? "loading…"), null, { timeout: 20000 });
  } catch {
    ok(false, `${tag} ${label}: the page did not draw (${errs.join(" | ")})`);
    continue;
  }
  if (label === "ticket") await page.waitForSelector("#conversation .cnew", { timeout: 20000 }).catch(() => {});
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(800);
  // The nav at its end, as the owner had it.
  await page.evaluate(() => { const t = document.querySelector("#nav .tree-scroll"); if (t) t.scrollTop = t.scrollHeight; });
  await page.waitForTimeout(300);
  const d = await doc();
  ok(d.sw <= d.iw, `${tag} ${label}: no horizontal scroll (${d.sw} <= ${d.iw})`);
  if (!phone) {
    const why = d.sh === d.ih ? [] : await culprits();
    ok(d.sh === d.ih, `${tag} ${label}: the document is the viewport's height (${d.sh} === ${d.ih})${why.length ? " — past it: " + why.join("; ") : ""}`);
    // A wheel at the end of the nav, then of the content: the window stays put.
    for (const [col, x] of [["nav", 180], ["content", 360 + (width - 360) / 2]]) {
      await page.evaluate((c) => { const s = c === "nav" ? document.querySelector("#nav .tree-scroll") : document.querySelector("#app .content"); if (s) s.scrollTop = s.scrollHeight; }, col);
      await page.mouse.move(x, height - 60);
      await page.mouse.wheel(0, 2000);
      await page.waitForTimeout(250);
      const y = await page.evaluate(() => scrollY);
      ok(y === 0, `${tag} ${label}: a wheel at the end of the ${col} moves nothing else (scrollY ${y})`);
    }
  } else {
    const layout = await page.evaluate(() => getComputedStyle(document.querySelector("#app .content")).overflowY);
    ok(layout === "visible", `${tag} ${label}: the phone's layout — the window is the scroller (.content overflow ${layout})`);
    if (label === "ticket") {
      ok(d.sh > d.ih, `${tag} ${label}: a long ticket scrolls the window on a phone (${d.sh} > ${d.ih})`);
      // It ends where the page does: the last of .content's flow, plus its padding.
      const end = await page.evaluate(() => {
        const c = document.querySelector("#app .content");
        return Math.round(c.getBoundingClientRect().bottom + scrollY);
      });
      ok(Math.abs(d.sh - end) <= 2, `${tag} ${label}: the window ends where the page does (${d.sh} vs ${end})`);
    }
  }
  if (shots) {
    await page.evaluate(() => window.scrollTo(0, document.scrollingElement.scrollHeight));
    await page.waitForTimeout(200);
    await page.screenshot({ path: `${shots}/${label}-${tag}.png` });
  }
}
ok(errs.length === 0, `${tag}: no page errors (${errs.join(" | ")})`);
await browser.close();
if (fails.length) { console.log(`${fails.length} failed`); process.exit(1); }
