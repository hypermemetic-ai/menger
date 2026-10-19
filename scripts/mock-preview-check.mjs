#!/usr/bin/env node
// APP-99: mockups on a phone — a preview at most a third of the screen,
// collapsible (remembered), a tap or ⤢ Open opens it full screen at 100%,
// Back or Exit closes it to the same scroll; at 1440 nothing changes. Light
// and dark, with the safe-area insets set through --sai-* (APP-66).
// Against a throwaway server holding a ticket TICKET with several mockups;
// scripts/throwaway-login.mjs makes the session file.
//
//   node scripts/mock-preview-check.mjs OUTDIR http://127.0.0.1:PORT SESSION_FILE TICKET [chromium|webkit]
//
// Exit 1 on a failure; OUTDIR gets a shot per step.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [out, origin, tokenFile, ticket, bname = "chromium"] = process.argv.slice(2);
fs.mkdirSync(out, { recursive: true });
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
// WebKit's notice when a box's height moves under its observers — main's own
// Full screen raises it too (checked 2026-10-19); no exception, nothing lost.
const BENIGN = /^ResizeObserver loop completed with undelivered notifications/;
const SAI = { top: 47, bottom: 34 };
const sai = (page) => page.addStyleTag({ content: `:root{--sai-top:${SAI.top}px;--sai-bottom:${SAI.bottom}px;--sai-left:0px;--sai-right:0px}` });

// The n-th mockup box's framed element, and what it measures.
const box = (page, n) => page.evaluate((n) => {
  const m = document.querySelectorAll(".mmock")[n];
  if (!m) return null;
  const f = m.querySelector(".framed");
  const st = f && f.querySelector(".framed-stage");
  const scr = f && f.querySelector(".device:not(.hidden) .device-screen");
  const bar = f && f.querySelector(".framed-bar");
  return {
    top: m.getBoundingClientRect().top,
    preview: f && f.hasAttribute("data-preview"),
    collapsed: f && f.hasAttribute("data-collapsed"),
    full: f && f.classList.contains("is-full"),
    stage: st ? st.getBoundingClientRect().height : 0,
    shown: [...m.querySelectorAll(".framed-stage, .framed-bar")].filter((e) => e.getClientRects().length).length,
    screenW: scr ? scr.getBoundingClientRect().width : 0,
    barTop: bar ? bar.getBoundingClientRect().top : 0,
    label: f && f.querySelector(".framed-full")?.textContent,
    collapsedLine: m.querySelector(".framed-collapsed")?.textContent || null,
    deviceLabel: !!(f && f.querySelector(".device:not(.hidden) .device-label") && getComputedStyle(f.querySelector(".device:not(.hidden) .device-label")).display !== "none"),
  };
}, n);

const browser = await pw[bname].launch();
for (const theme of ["light", "dark"]) {
  const tag = `${bname} 390 ${theme}`;
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, hasTouch: true, isMobile: bname !== "firefox", colorScheme: theme });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => { if (!BENIGN.test(String(e))) errs.push(String(e)); });
  await page.goto(`${origin}/#/ticket/${ticket}`);
  await page.evaluate(() => { for (const k of Object.keys(localStorage)) if (k.startsWith("menger.mockup.folded.")) localStorage.removeItem(k); });
  await page.waitForSelector(".mmock .framed[data-preview] .device iframe", { timeout: 30000 });
  await page.waitForTimeout(2500);
  await sai(page);
  await page.waitForTimeout(300);
  const n = await page.$$eval(".mmock", (e) => e.length);
  ok(n >= 2, `${tag}: ${n} mockups`);
  for (let i = 0; i < n; i++) {
    const b = await box(page, i);
    ok(b.preview && b.stage > 40 && b.stage <= 844 / 3 + 0.5, `${tag}: mockup ${i + 1} is a preview ${b.stage.toFixed(1)} px tall (≤ ${(844 / 3).toFixed(1)})`);
    ok(!b.deviceLabel && b.label === "⤢ Open", `${tag}: mockup ${i + 1} unlabelled, its button ⤢ Open (${b.label})`);
  }
  // The second mockup, scrolled into view: a tap opens it full screen.
  await page.locator(".mmock").nth(1).scrollIntoViewIfNeeded();
  await page.waitForTimeout(400);
  const before = await box(page, 1);
  await page.screenshot({ path: `${out}/${tag.replaceAll(" ", "-")}-preview.png` });
  await page.locator(".mmock").nth(1).locator(".framed-stage").tap();
  await page.waitForTimeout(800);
  let b = await box(page, 1);
  ok(b.full && Math.abs(b.screenW - 390) < 1.5, `${tag}: a tap opens it full screen at 100% (screen ${b.screenW.toFixed(1)} px)`);
  ok(b.barTop >= SAI.top - 0.5, `${tag}: the full screen's bar sits under the top inset (${b.barTop.toFixed(1)})`);
  await page.screenshot({ path: `${out}/${tag.replaceAll(" ", "-")}-full.png` });
  await page.goBack();
  await page.waitForTimeout(800);
  b = await box(page, 1);
  ok(!b.full && Math.abs(b.top - before.top) < 1.5 && page.url().includes(`/ticket/${ticket}`), `${tag}: Back closes it to the same scroll (top ${before.top.toFixed(1)} → ${b.top.toFixed(1)})`);
  // ⤢ Open, then Exit full screen.
  await page.locator(".mmock").nth(1).locator(".framed-full").tap();
  await page.waitForTimeout(600);
  ok((await box(page, 1)).full, `${tag}: ⤢ Open opens it`);
  await page.locator(".mmock").nth(1).locator(".framed-full").tap();
  await page.waitForTimeout(800);
  b = await box(page, 1);
  ok(!b.full && Math.abs(b.top - before.top) < 1.5, `${tag}: Exit closes it to the same scroll (${b.top.toFixed(1)})`);
  const len = await page.evaluate(() => history.length);
  // Collapse: one line, kept across a reload.
  await page.locator(".mmock").nth(1).locator(".framed-collapse").tap();
  await page.waitForTimeout(600);
  b = await box(page, 1);
  const line = await page.locator(".mmock").nth(1).evaluate((m) => m.getBoundingClientRect().height);
  ok(b.collapsed && b.shown === 0 && line < 60 && /▸/.test(b.collapsedLine || "") && /2 views/.test(b.collapsedLine || ""), `${tag}: collapsed to one line, ${line.toFixed(1)} px, nothing else drawn (${b.collapsedLine})`);
  await page.screenshot({ path: `${out}/${tag.replaceAll(" ", "-")}-collapsed.png` });
  await page.reload();
  await page.waitForSelector(".mmock .framed", { timeout: 30000 });
  await page.waitForTimeout(2500);
  b = await box(page, 1);
  ok(b.collapsed && !(await box(page, 0)).collapsed, `${tag}: the collapse is remembered, for that mockup alone`);
  await page.locator(".mmock").nth(1).locator(".framed-collapsed").tap();
  await page.waitForTimeout(800);
  b = await box(page, 1);
  ok(!b.collapsed && b.preview, `${tag}: ▸ opens it again (a preview)`);
  ok(len >= 1, `${tag}: history ${len}`);
  ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
  await ctx.close();
}
// 1440: inline and interactive, as before; collapse there too.
for (const theme of ["light", "dark"]) {
  const tag = `${bname} 1440 ${theme}`;
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: theme });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => { if (!BENIGN.test(String(e))) errs.push(String(e)); });
  await page.goto(`${origin}/#/ticket/${ticket}`);
  await page.waitForSelector(".mmock .framed .device iframe", { timeout: 30000 });
  await page.waitForTimeout(2500);
  const b = await box(page, 0);
  const devices = await page.$$eval(".mmock:first-of-type .device:not(.hidden)", (e) => e.length).catch(() => 0);
  ok(!b.preview && !b.full && b.deviceLabel && b.label === "⤢ Full screen", `${tag}: no preview, labelled devices, ⤢ Full screen (${b.label}, ${devices} devices)`);
  const pe = await page.$eval(".mmock .device-body", (e) => getComputedStyle(e).pointerEvents);
  ok(pe !== "none", `${tag}: the devices take the pointer (${pe})`);
  await page.screenshot({ path: `${out}/${tag.replaceAll(" ", "-")}.png` });
  await page.locator(".mmock").first().locator(".framed-collapse").click();
  await page.waitForTimeout(500);
  const c = await box(page, 0);
  ok(c.collapsed && c.shown === 0, `${tag}: collapse is there too`);
  await page.locator(".mmock").first().locator(".framed-collapsed").click();
  await page.waitForTimeout(500);
  ok(!(await box(page, 0)).collapsed, `${tag}: and opens again`);
  ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
  await ctx.close();
}
await browser.close();
console.log(fails.length ? `${fails.length} failed` : "all ok");
process.exit(fails.length ? 1 : 0);
