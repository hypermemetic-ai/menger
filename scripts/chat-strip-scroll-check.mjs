#!/usr/bin/env node
// CHAT-60: the ticket strip goes with the chat's head, in a browser, at 390
// (iPhone 13, Chromium). Against a throwaway (scripts/chat-strip-scroll-check.sh):
// the reader scrolls down — the head and the strip are both off-screen;
// up — both are back, the strip under the head; the turns never move in
// the document (no layout shift); with a pill open, a scroll down moves
// neither; no page errors.
//
//   node scripts/chat-strip-scroll-check.mjs ORIGIN SESSION_FILE CHAT THEME [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat, scheme, shots] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: scheme });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const tag = `390-${scheme}`;
await page.goto(`${origin}/app/#/chats/${chat}`);
try {
  await page.waitForSelector("#chatfull .cstrip button", { timeout: 30000 });
  await page.waitForSelector("#chatfull .trow", { timeout: 30000 });
} catch (e) {
  if (shots) await page.screenshot({ path: `${shots}/${tag}-no-chat.png` });
  console.log(`FAIL ${tag}: the chat and its strip never drew (${errs.join(" | ")})`);
  process.exit(1);
}
await page.waitForTimeout(1500);

// A reader's scroll: a wheel (the reader's hand, src/chat_full.rs) then the move.
const scroll = async (dy) => {
  await page.evaluate((dy) => { window.dispatchEvent(new WheelEvent("wheel", { deltaY: dy })); window.scrollBy(0, dy); }, dy);
  await page.waitForTimeout(450);
};
const where = () => page.evaluate(() => {
  const r = (s) => { const e = document.querySelector(s); if (!e) return null; const b = e.getBoundingClientRect(); return { top: b.top, bottom: b.bottom }; };
  // One turn, marked once: where it sits in the document.
  let mid = document.querySelector("#chatfull .trow[data-probe]");
  if (!mid) {
    mid = [...document.querySelectorAll("#chatfull .trow")].find((r) => r.getBoundingClientRect().top > innerHeight / 2);
    mid?.setAttribute("data-probe", "");
  }
  return { head: r("#chatfull .shead"), strip: r("#chatfull .cstrip"), y: scrollY, max: document.documentElement.scrollHeight - innerHeight, row: mid ? mid.getBoundingClientRect().top + scrollY : null, off: document.querySelector("#chatfull").getAttribute("data-head") };
});

let w = await where();
ok(w.max > 600, `${tag}: the chat scrolls the document (${Math.round(w.max)} px)`);
// Up from the landing at the bottom, well past the slop: the head shows.
await scroll(-500);
w = await where();
ok(w.off !== "off" && w.head.top >= -1 && Math.abs(w.strip.top - w.head.bottom) < 2, `${tag}: at rest the strip sits under the head (head ${Math.round(w.head.bottom)}, strip ${Math.round(w.strip.top)})`);
const row0 = w.row;
if (shots) await page.screenshot({ path: `${shots}/${tag}-shown.png` });
// Down: both slide away.
await scroll(200);
w = await where();
ok(w.off === "off", `${tag}: down — the head hides`);
ok(w.head.bottom <= 1 && w.strip.bottom <= 1, `${tag}: down — the head and the strip are both off-screen (head ${Math.round(w.head.bottom)}, strip ${Math.round(w.strip.bottom)})`);
ok(Math.abs(w.row - row0) < 1, `${tag}: down — the turns did not move (${row0} → ${w.row})`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-hidden.png` });
// Up: both come back.
await scroll(-120);
w = await where();
ok(w.off !== "off" && w.head.top >= -1 && Math.abs(w.strip.top - w.head.bottom) < 2, `${tag}: up — both are back, the strip under the head (head ${Math.round(w.head.bottom)}, strip ${Math.round(w.strip.top)})`);
ok(Math.abs(w.row - row0) < 1, `${tag}: up — the turns did not move (${row0} → ${w.row})`);
// A pill open holds them.
await page.click("#chatfull .cstrip button >> nth=0");
await page.waitForSelector("#chatfull .cdrop", { timeout: 5000 });
await scroll(200);
w = await where();
ok(w.off !== "off" && w.strip.top >= 0, `${tag}: a pill open — the strip and the head stay put`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-open.png` });
await page.keyboard.press("Escape");
await page.waitForTimeout(200);
await scroll(200);
w = await where();
ok(w.off === "off" && w.strip.bottom <= 1, `${tag}: closed again — a scroll down takes both away`);
ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
await browser.close();
if (fails.length) { console.log(`${fails.length} failed`); process.exit(1); }
console.log("all ok");
