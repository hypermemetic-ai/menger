#!/usr/bin/env node
// CHAT-55: the chat's ticket strip, in a browser. Against a throwaway server
// holding chat CHAT whose messages name tickets in several stages, and chat
// EMPTY naming none; scripts/throwaway-login.mjs makes the session file. At
// 390 (iPhone 13; Chromium, or WebKit with PHONE_ENGINE=webkit) and 1440 (Chromium), light and dark: the strip is
// under the head with a pill per stage and its count; a pill opens its
// tickets (id, title, status) and closes again (the pill; Escape; on a
// phone a swipe up); nothing interactive is clipped; no page errors; EMPTY
// has no strip. With MOVE set, it runs MOVE (a shell command that moves a
// ticket's stage) and waits for the pill named by MOVED_STAGE to show
// MOVED_COUNT — live, without a reload. Shots to SHOTS when set.
//
//   node scripts/chat-strip-check.mjs http://127.0.0.1:PORT SESSION_FILE CHAT EMPTY
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
import { execSync } from "node:child_process";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat, empty] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const shots = process.env.SHOTS;
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const pills = (page) => page.$$eval(".cstrip button", (bs) => bs.map((b) => `${b.dataset.stage}=${b.querySelector("b")?.textContent}`));

async function run(engine, device, scheme) {
  const browser = await pw[engine].launch();
  const ctx = await browser.newContext({ ...device, colorScheme: scheme });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  const w = device.viewport.width;
  const tag = `${w}-${scheme}`;
  await page.goto(`${origin}/#/chats/${chat}`);
  await page.waitForSelector("#chatfull .cstrip button", { timeout: 30000 });
  await page.waitForTimeout(1500);
  const ps = await pills(page);
  ok(ps.length >= 3, `${tag}: a pill per stage held (${ps.join(" ")})`);
  ok(ps[0]?.startsWith("waits_on_you="), `${tag}: waits on you first`);
  const strip = await page.$eval(".cstrip", (s) => { const r = s.getBoundingClientRect(); return { top: r.top, h: r.height, w: r.width }; });
  ok(strip.top >= 0 && strip.top < 160, `${tag}: the strip is under the head (top ${Math.round(strip.top)})`);
  if (w < 500) ok(strip.h <= 56, `${tag}: one row on a phone (${Math.round(strip.h)}px)`);
  if (shots) await page.screenshot({ path: `${shots}/${tag}-rest.png` });
  // Open the first pill: its tickets.
  await page.click(".cstrip button >> nth=0");
  await page.waitForSelector(".cdrop .cdrow", { timeout: 5000 });
  const rows = await page.$$eval(".cdrop .cdrow", (rs) => rs.map((r) => r.querySelector(".code")?.textContent));
  const n0 = Number(ps[0].split("=")[1]);
  ok(rows.length === n0, `${tag}: the pill opens its ${n0} tickets (${rows.join(", ")})`);
  ok(await page.$eval(".cstrip button >> nth=0", (b) => b.getAttribute("aria-expanded")) === "true", `${tag}: the open pill says so`);
  const drop = await page.$eval(".cdrop", (d) => { const r = d.getBoundingClientRect(); return { h: r.height, bottom: r.bottom }; });
  const vh = device.viewport.height;
  if (w < 500) ok(drop.h <= vh * 0.5 + 1, `${tag}: the drop is at most half the screen (${Math.round(drop.h)} of ${vh})`);
  ok(drop.bottom <= vh, `${tag}: the drop is on screen`);
  if (shots) await page.screenshot({ path: `${shots}/${tag}-open.png` });
  // Close: the pill again, then Escape, then (phone) a swipe up.
  await page.click(".cstrip button >> nth=0");
  ok(!(await page.$(".cdrop")), `${tag}: the pill again closes it`);
  await page.click(".cstrip button >> nth=0");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  ok(!(await page.$(".cdrop")), `${tag}: Escape closes it`);
  if (w < 500) {
    await page.click(".cstrip button >> nth=0");
    const box = await page.$eval(".cdrop", (d) => { const r = d.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height * 0.7 }; });
    await page.$eval(".cdrop", (d, b) => {
      const ev = (type, y) => d.dispatchEvent(new PointerEvent(type, { bubbles: true, clientX: b.x, clientY: y, pointerType: "touch" }));
      ev("pointerdown", b.y); ev("pointerup", b.y - 120);
    }, box);
    await page.waitForTimeout(200);
    ok(!(await page.$(".cdrop")), `${tag}: a swipe up closes it`);
  }
  // A row opens its ticket.
  await page.click(".cstrip button >> nth=0");
  const href = await page.$eval(".cdrop .cdrow", (a) => a.getAttribute("href"));
  ok(/^#\/ticket\/[A-Z]+-\d+$/.test(href ?? ""), `${tag}: a row links its ticket (${href})`);
  await page.keyboard.press("Escape");
  // Live: a move elsewhere moves a pill, no reload.
  if (process.env.MOVE && process.env.MOVE_AT === tag) {
    execSync(process.env.MOVE, { stdio: "ignore" });
    const want = `${process.env.MOVED_STAGE}=${process.env.MOVED_COUNT}`;
    let got = [];
    for (let i = 0; i < 40; i++) {
      got = await pills(page);
      if (got.includes(want)) break;
      await page.waitForTimeout(250);
    }
    ok(got.includes(want), `${tag}: live, ${want} (${got.join(" ")})`);
    if (shots) await page.screenshot({ path: `${shots}/${tag}-moved.png` });
  }
  // A chat naming no ticket: no strip.
  await page.goto(`${origin}/#/chats/${empty}`);
  await page.waitForSelector("#chatfull .chatview", { timeout: 30000 });
  await page.waitForTimeout(1500);
  ok(!(await page.$(".cstrip")), `${tag}: a chat with no tickets has no strip`);
  ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
  await browser.close();
}

const phone = pw.devices["iPhone 13"];
const wide = { viewport: { width: 1440, height: 900 } };
for (const scheme of ["light", "dark"]) {
  await run(process.env.PHONE_ENGINE ?? "chromium", phone, scheme);
  await run("chromium", wide, scheme);
}
if (fails.length) { console.log(`${fails.length} failed`); process.exit(1); }
console.log("all ok");
