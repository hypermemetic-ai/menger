#!/usr/bin/env node
// APP-65 / APP-66: the app on an iPhone's screen. The phone bar, the tray's
// pill, the chat's composer and the note under it must clear the rounded
// corners and the home indicator, and a long inline code span stay inside
// the body. Headless browsers report env(safe-area-inset-*) as 0, so the
// check sets the insets through the app's --sai-* (the contract's --safe-*
// read them, as a mockup's phone does) and draws the screen's shape over
// each shot. Against a throwaway server (scripts/throwaway-login.mjs makes
// the session file); a ticket with a `media:<hex>` code span, a chat:
//   node scripts/safe-area-check.mjs chromium OUTDIR http://127.0.0.1:PORT SESSION_FILE TICKET CHAT [sai|none]
// WebKit runs in the Playwright image (docker, --network host). Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [bname, out, origin, tokenFile, ticket, chat, mode = "sai"] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };

// iPhone 13 (390×844, corner ≈ 47 pt, notch, top 47) and 15 Pro (393×852,
// corner ≈ 55 pt, island, top 59); bottom 34 in portrait for both.
const DEVICES = [
  { name: "iphone13", w: 390, h: 844, r: 47, top: 47, bottom: 34 },
  { name: "iphone15pro", w: 393, h: 852, r: 55, top: 59, bottom: 34 },
];
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

// A point is inside the screen's rounded rectangle.
function inside(x, y, d) {
  const r = d.r;
  const cx = x < r ? r : x > d.w - r ? d.w - r : x;
  const cy = y < r ? r : y > d.h - r ? d.h - r : y;
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r + 0.5;
}
function clear(rect, d) {
  const pts = [[rect.x, rect.y], [rect.x + rect.width, rect.y], [rect.x, rect.y + rect.height], [rect.x + rect.width, rect.y + rect.height]];
  return pts.every(([x, y]) => inside(x, y, d)) && rect.y + rect.height <= d.h - d.bottom + 0.5;
}

async function emulate(page, d) {
  if (mode === "none") return;
  const p = mode === "sai" ? "sai" : "safe";
  await page.addStyleTag({ content: `:root{--${p}-top:${d.top}px;--${p}-bottom:${d.bottom}px;--${p}-left:0px;--${p}-right:0px}` });
}
// The screen's shape drawn over the shot: corners, the island, the home indicator.
async function overlay(page, d) {
  await page.evaluate((d) => {
    const o = document.createElement("div");
    o.style.cssText = `position:fixed;inset:0;border-radius:${d.r}px;box-shadow:0 0 0 200px #000;pointer-events:none;z-index:2147483647`;
    const i = document.createElement("div");
    i.style.cssText = `position:fixed;left:50%;top:11px;width:126px;height:37px;margin-left:-63px;border-radius:20px;background:#000;pointer-events:none;z-index:2147483647`;
    const h = document.createElement("div");
    h.style.cssText = `position:fixed;left:50%;bottom:8px;width:134px;height:5px;margin-left:-67px;border-radius:3px;background:#888;pointer-events:none;z-index:2147483647`;
    document.documentElement.append(o, i, h);
  }, d);
}

const browser = await pw[bname].launch();
for (const theme of ["light", "dark"]) {
  for (const d of DEVICES) {
    const ctx = await browser.newContext({
      viewport: { width: d.w, height: d.h }, deviceScaleFactor: 2, hasTouch: true, isMobile: bname !== "firefox",
      colorScheme: theme,
    });
    await ctx.addCookies([cookie]);
    const page = await ctx.newPage();
    const errs = [];
    page.on("pageerror", (e) => errs.push(String(e)));
    const tag = `${bname} ${d.name} ${theme}`;

    // The ticket page: the bar, the tray's pill, the body's code span.
    await page.goto(`${origin}/#/ticket/${ticket}`);
    await page.waitForSelector("#pbar .pl", { timeout: 20000 });
    await page.waitForTimeout(2500);
    await emulate(page, d);
    await page.waitForTimeout(300);
    const meta = await page.getAttribute('meta[name="viewport"]', "content");
    ok(/viewport-fit=cover/.test(meta), `${tag}: viewport-fit=cover (${meta})`);
    const declared = await page.evaluate(() => {
      for (const s of document.styleSheets) for (const r of s.cssRules) {
        const v = r.style && r.style.getPropertyValue("--safe-bottom");
        if (v) return v.trim();
      }
      return "";
    });
    ok(/env\(safe-area-inset-bottom/.test(declared), `${tag}: --safe-bottom reads env() (${declared})`);
    const pls = await page.$$eval("#pbar .pl", (es) => es.map((e) => e.getBoundingClientRect().toJSON()));
    ok(pls.length === 4 && pls.every((r) => clear(r, d)), `${tag}: the bar's 4 places clear the corners and home indicator`);
    const qc = await page.$eval("#qtray .qc", (e) => e.getBoundingClientRect().toJSON()).catch(() => null);
    if (qc && qc.width) ok(clear(qc, d), `${tag}: the tray's control clears them`);
    // The body's frame: no inline code past its card.
    const fr = page.frames().find((f) => f !== page.mainFrame() && /frame/.test(f.url()));
    if (fr) {
      const m = await fr.evaluate(() => {
        const c = [...document.querySelectorAll("code")].find((e) => !e.closest("pre"));
        const b = document.body.getBoundingClientRect();
        const r = c ? c.getBoundingClientRect() : null;
        return { r: r && r.right, w: b.right, sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth };
      });
      ok(m.r !== null && m.r <= m.w + 0.5, `${tag}: the code span stays inside the body (right ${m.r?.toFixed(1)} ≤ ${m.w.toFixed(1)})`);
    } else {
      const m = await page.evaluate(() => {
        const c = [...document.querySelectorAll(".md code")].find((e) => !e.closest("pre") && /media:/.test(e.textContent));
        const md = c && c.closest(".md").getBoundingClientRect();
        const pre = document.querySelector(".md pre");
        return { r: c ? c.getBoundingClientRect().right : null, w: md ? md.right : 0, pre: pre ? pre.getBoundingClientRect().right : 0, sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth };
      });
      ok(m.r !== null && m.r <= m.w + 0.5, `${tag}: the code span stays inside the body (right ${m.r?.toFixed(1)} ≤ ${m.w.toFixed(1)})`);
      ok(m.pre <= m.w + 0.5 && m.sw <= m.cw, `${tag}: the code block scrolls inside it, the page does not scroll sideways (${m.sw} ≤ ${m.cw})`);
    }
    await overlay(page, d);
    await page.screenshot({ path: `${out}/${bname}-${d.name}-${theme}-ticket.png` });

    // The full-screen chat: the composer.
    await page.goto(`${origin}/#/chats/${chat}`);
    await page.waitForSelector("#chatfull .ccompose .ccard", { timeout: 20000 });
    await page.waitForTimeout(1500);
    await emulate(page, d);
    await page.waitForTimeout(300);
    const cc = await page.$eval("#chatfull .ccompose .ccard", (e) => e.getBoundingClientRect().toJSON());
    ok(clear(cc, d), `${tag}: the composer clears the corners and home indicator (bottom ${(cc.y + cc.height).toFixed(1)} of ${d.h})`);
    const sf = await page.$eval("#chatfull .sfoot", (e) => { const r = document.createRange(); r.selectNodeContents(e); return r.getBoundingClientRect().toJSON(); }).catch(() => null);
    if (sf) ok(clear(sf, d), `${tag}: the note under the composer clears them too (bottom ${(sf.y + sf.height).toFixed(1)})`);
    const sh = await page.$eval("#chatfull .shead", (e) => e.getBoundingClientRect().toJSON());
    ok(sh.y >= (mode === "none" ? 0 : d.top) - 0.5, `${tag}: the chat's head sits under the top inset (${sh.y.toFixed(1)})`);
    await overlay(page, d);
    await page.screenshot({ path: `${out}/${bname}-${d.name}-${theme}-chat.png` });
    ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
    await ctx.close();
  }
}
// 1440: no insets, nothing moved.
{
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  await page.goto(`${origin}/#/ticket/${ticket}`);
  await page.waitForTimeout(3000);
  const pad = await page.$eval("#app", (e) => getComputedStyle(e).paddingLeft + " " + getComputedStyle(e).paddingRight);
  ok(pad === "0px 0px", `${bname} 1440: #app's sides unpadded (${pad})`);
  await page.screenshot({ path: `${out}/${bname}-1440-ticket.png` });
  await ctx.close();
}
await browser.close();
console.log(fails.length ? `${fails.length} FAILED` : "all ok");
process.exit(fails.length ? 1 : 0);
