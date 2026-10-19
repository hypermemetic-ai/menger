#!/usr/bin/env node
// CHAT-52: the full chat's composer on an iPhone clears the home indicator
// and the screen's corners with the keyboard down, and sits on the keyboard
// with no gap when it is up. WebKit at 390 as an iPhone 13 (touch, so the
// phone's `(pointer: coarse)` rules apply — CHAT-29's one-line composer,
// whose padding took the inset away), light and dark; the insets through
// the app's --sai-* (headless WebKit reports env() as 0), the keyboard
// through scripts/kb-shim.js.
//
// Against a throwaway server holding a chat CHAT long enough to scroll
// (scripts/throwaway-login.mjs makes the session file):
//
//   node scripts/composer-inset-check.mjs OUTDIR http://127.0.0.1:PORT SESSION_FILE CHAT
//
// WebKit runs in the Playwright image (docker, --network host). Exit 1 on a
// failure; OUTDIR gets a shot per case, the screen's shape drawn over it.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [out, origin, tokenFile, chat] = process.argv.slice(2);
fs.mkdirSync(out, { recursive: true });
const shim = fs.readFileSync(new URL("./kb-shim.js", import.meta.url), "utf8");
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${u.port}`, secure: u.protocol === "https:", value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const f1 = (n) => (n == null ? "∅" : n.toFixed(1));

// iPhone 13: 390 × 844, corner ≈ 47, top 47, bottom 34.
const D = { w: 390, h: 844, r: 47, top: 47, bottom: 34 };
const inCorner = (x, y) => {
  const cx = x < D.r ? D.r : x > D.w - D.r ? D.w - D.r : x;
  const cy = y < D.r ? D.r : y > D.h - D.r ? D.h - D.r : y;
  return (x - cx) ** 2 + (y - cy) ** 2 <= D.r * D.r + 0.5;
};
// The card's four corners, inset by its own rounding: the drawn edge, not
// its bounding box, must be inside the screen.
const cornersIn = (c) => {
  const k = c.radius * (1 - Math.SQRT1_2);
  return [[c.left + k, c.top + k], [c.right - k, c.top + k], [c.left + k, c.bottom - k], [c.right - k, c.bottom - k]].every(([x, y]) => inCorner(x, y));
};

const card = (page) => page.evaluate(() => {
  const e = document.querySelector("#chatfull .ccompose .ccard");
  if (!e) return null;
  const b = e.getBoundingClientRect(); // border included
  const pad = getComputedStyle(e.closest(".ccompose")).paddingBottom;
  return { top: b.top, bottom: b.bottom, left: b.left, right: b.right, radius: parseFloat(getComputedStyle(e).borderBottomLeftRadius) || 0, pad, kb: window.__kb.state(), dataKb: document.documentElement.hasAttribute("data-kb") };
});

async function overlay(page, kb) {
  await page.evaluate(({ d, kb }) => {
    for (const e of document.querySelectorAll(".c52-draw")) e.remove();
    const mk = (css, text = "") => { const e = document.createElement("div"); e.className = "c52-draw"; e.style.cssText = `pointer-events:none;z-index:2147483647;${css}`; e.textContent = text; document.documentElement.append(e); };
    mk(`position:fixed;inset:0;border-radius:${d.r}px;box-shadow:0 0 0 200px #000`);
    mk(`position:fixed;left:50%;top:11px;width:126px;height:37px;margin-left:-63px;border-radius:20px;background:#000`);
    if (kb && kb.kb > 0) mk(`position:fixed;left:0;right:0;top:${kb.bottom}px;height:${kb.kb}px;background:#3a3a3c;color:#fff;font:14px sans-serif;display:grid;place-items:center`, "keyboard");
    else mk(`position:fixed;left:50%;bottom:8px;width:134px;height:5px;margin-left:-67px;border-radius:3px;background:#888`);
  }, { d: D, kb });
}

const browser = await pw.webkit.launch();
for (const theme of ["light", "dark"]) {
  // The whole screen, Safari's bars collapsed (the owner's shot): 390 × 844.
  const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], viewport: { width: D.w, height: D.h }, colorScheme: theme });
  await ctx.addCookies([cookie]);
  await ctx.addInitScript(`window.__kbConf = { k: 336, ms: 300 };\n${shim}`);
  await ctx.addInitScript(`document.addEventListener("DOMContentLoaded", () => { const s = document.createElement("style"); s.textContent = ":root{--sai-top:${D.top}px;--sai-bottom:${D.bottom}px;--sai-left:0px;--sai-right:0px}"; document.head.append(s); });`);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  const tag = `webkit 390 ${theme}`;
  ok(await page.evaluate(() => matchMedia("(pointer: coarse)").matches), `${tag}: the phone's touch rules apply (pointer: coarse)`);

  await page.goto(`${origin}/#/chats/${chat}`);
  await page.waitForSelector("#chatfull .ccompose .ccard", { timeout: 30000 });
  await page.waitForTimeout(2000);
  ok(await page.evaluate((h) => document.documentElement.clientHeight === h, D.h), `${tag}: the layout viewport is the screen (${D.h})`);
  // At rest at the thread's end, and up the thread: the sticky composer
  // is pinned to the screen's bottom either way.
  for (const where of ["end", "back"]) {
    await page.evaluate((w) => window.__kb.drag(w === "end" ? 1e6 : -1200), where);
    await page.waitForTimeout(600);
    const c = await card(page);
    const t = `${tag}, keyboard down, ${where === "end" ? "at the end" : "read back"}`;
    ok(c && !c.dataKb, `${t}: no keyboard`);
    ok(c && c.bottom <= D.h - D.bottom + 0.5, `${t}: the card, border included, ends ≥ ${D.bottom} px above the bottom (${f1(c?.bottom)} ≤ ${D.h - D.bottom}; the foot's padding ${c?.pad})`);
    ok(c && cornersIn(c), `${t}: its rounded corners are inside the screen's`);
    await overlay(page, null);
    await page.screenshot({ path: `${out}/${theme}-down-${where}.png` });
    await page.evaluate(() => { for (const e of document.querySelectorAll(".c52-draw")) e.remove(); });
  }

  // The keyboard up: on it, no inset under it.
  await page.tap("#chatfull textarea.cbox");
  await page.waitForTimeout(1200);
  const c = await card(page);
  const t = `${tag}, keyboard up`;
  ok(c && c.dataKb, `${t}: data-kb is set`);
  const gap = c ? c.kb.bottom - c.bottom : null;
  ok(c && gap >= 0 && gap <= 8, `${t}: the card sits on the keyboard, no inset under it (gap ${f1(gap)} px, keyboard at ${f1(c?.kb.bottom)}; the foot's padding ${c?.pad})`);
  ok(c && c.top >= c.kb.top, `${t}: the card is in the visible area (${f1(c?.top)} ≥ ${f1(c?.kb.top)})`);
  await overlay(page, c?.kb);
  await page.screenshot({ path: `${out}/${theme}-up.png` });
  ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
  await ctx.close();
}
await browser.close();
console.log(fails.length ? `${fails.length} FAILED` : "all ok");
process.exit(fails.length ? 1 : 0);
