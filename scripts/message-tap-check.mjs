#!/usr/bin/env node
// CHAT-53: on a phone a plain tap on a chat message does nothing to the
// message — no `held`, no revealed menu (iOS keeps :hover after a tap), no
// focus, no native selection, no highlight — while a long press still opens
// its menu, "select" still enters selection mode (a tap then picks), and a
// tap on a chip still opens it in one tap. WebKit at 390 (iPhone 13, touch),
// light and dark. Against a throwaway server holding a chat CHAT whose
// newest messages hold prose, one of them a ticket code
// (scripts/throwaway-login.mjs makes the session file):
//
//   node scripts/message-tap-check.mjs http://127.0.0.1:PORT SESSION_FILE CHAT
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${u.port}`, secure: u.protocol === "https:", value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

// Everything a tap could do to a turn, read back.
const probe = (page, sel) => page.evaluate((sel) => {
  const el = document.querySelector(sel);
  const turn = el.closest(".turn");
  const cs = getComputedStyle(turn);
  const acts = turn.querySelector(".tacts");
  const sx = getSelection();
  return {
    cls: turn.className,
    bg: cs.backgroundColor,
    outline: cs.outlineStyle + cs.outlineWidth,
    actsOpacity: acts ? getComputedStyle(acts).opacity : null,
    focusIn: turn.contains(document.activeElement) && document.activeElement !== document.body,
    selected: sx.toString(),
    selecting: !!document.querySelector(".chatview.selecting"),
  };
}, sel);

// A long press as a finger does it: touch pointer events, 650 ms apart.
async function longPress(page, sel) {
  await page.evaluate((sel) => {
    const el = document.querySelector(sel);
    const r = el.getBoundingClientRect();
    const o = { bubbles: true, cancelable: true, composed: true, pointerType: "touch", pointerId: 7, isPrimary: true, clientX: r.x + 12, clientY: r.y + r.height / 2 };
    el.dispatchEvent(new PointerEvent("pointerdown", o));
  }, sel);
  await page.waitForTimeout(700);
}
async function release(page, sel) {
  await page.evaluate((sel) => {
    const el = document.querySelector(sel);
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, composed: true, pointerType: "touch", pointerId: 7, isPrimary: true, clientX: r.x + 12, clientY: r.y + r.height / 2 }));
  }, sel);
}

const browser = await pw.webkit.launch();
for (const scheme of ["light", "dark"]) {
  const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: scheme });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  await page.goto(`${origin}/#/chats/${chat}`);
  await page.waitForSelector("#chatfull .trow", { timeout: 30000 });
  await page.waitForTimeout(2000);
  // a message of somebody else's (it has the menu row) with plain text
  const text = "#chatfull .turn.theirs .bubble, #chatfull .turn.theirs .pmb";
  const sel = "#chatfull .turn.theirs:last-of-type .pmb, #chatfull .turn.theirs:last-of-type .bubble";
  await page.evaluate(() => document.querySelector("#chatfull .pmsgs").scrollTo(0, 1e7));
  await page.waitForTimeout(500);
  const pick = (await page.$$(text)).length ? text : sel;
  await page.evaluate((s) => { const all = [...document.querySelectorAll(s)]; all.at(-1).id = "tapme"; }, pick);
  const before = await probe(page, "#tapme");
  await page.tap("#tapme", { position: { x: 20, y: 8 } });
  await page.waitForTimeout(900);
  const after = await probe(page, "#tapme");
  const tag = `${scheme}: tap`;
  ok(!/\bheld\b/.test(after.cls), `${tag}: the turn is not held (${after.cls})`);
  ok(after.cls === before.cls, `${tag}: the turn's classes are unchanged`);
  ok(after.bg === before.bg, `${tag}: no highlight (${before.bg} → ${after.bg})`);
  ok(after.actsOpacity === before.actsOpacity, `${tag}: the menu row stays hidden (${before.actsOpacity} → ${after.actsOpacity})`);
  ok(after.outline === before.outline && !after.focusIn, `${tag}: no focus ring, no focus in the turn`);
  ok(after.selected === "", `${tag}: no native selection (${JSON.stringify(after.selected)})`);
  ok(!after.selecting, `${tag}: not in selection mode`);
  // a second tap a while later, after the first could have timed out
  await page.waitForTimeout(700);
  ok(!/\bheld\b/.test((await probe(page, "#tapme")).cls), `${tag}: still not held 700 ms later`);

  // long press: the menu
  await longPress(page, "#tapme");
  const lp = await probe(page, "#tapme");
  ok(/\bheld\b/.test(lp.cls) && lp.actsOpacity === "1", `${scheme}: a long press opens the menu (${lp.cls}, opacity ${lp.actsOpacity})`);
  await release(page, "#tapme");
  // select -> selection mode -> a tap picks
  const selectBtn = await page.$('#tapme >> xpath=ancestor::*[contains(@class,"turn")][1] >> [data-act="select"]');
  ok(!!selectBtn, `${scheme}: the menu has select`);
  if (selectBtn) {
    await selectBtn.evaluate((b) => b.click());
    await page.waitForTimeout(400);
    ok(await page.evaluate(() => !!document.querySelector(".chatview.selecting")), `${scheme}: select enters selection mode`);
    const picked = async () => page.evaluate(() => document.querySelectorAll(".trow.picked").length);
    const n0 = await picked();
    await page.tap("#tapme", { position: { x: 20, y: 8 } });
    await page.waitForTimeout(400);
    const n1 = await picked();
    ok(n1 !== n0, `${scheme}: in selection mode a tap picks (${n0} → ${n1})`);
  }
  ok(errs.length === 0, `${scheme}: no page errors ${errs.join(" | ")}`);
  await ctx.close();

  // a chip in a message still opens in one tap
  const c2 = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: scheme });
  await c2.addCookies([cookie]);
  const p2 = await c2.newPage();
  await p2.goto(`${origin}/#/chats/${chat}`);
  await p2.waitForSelector("#chatfull a.tchip", { timeout: 30000 });
  await p2.waitForTimeout(1500);
  await p2.tap("#chatfull a.tchip >> nth=-1");
  await p2.waitForTimeout(1000);
  const h = await p2.evaluate(() => location.hash);
  ok(/^#\/ticket\//.test(h), `${scheme}: one tap on a chip opens the ticket (${h})`);
  await c2.close();
}
await browser.close();
console.log(`\n${fails.length === 0 ? "PASS" : `FAIL ${fails.length}`}`);
process.exit(fails.length === 0 ? 0 : 1);
