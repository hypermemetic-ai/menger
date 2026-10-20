#!/usr/bin/env node
// CHAT-66: replying to a message, in a browser, at 390 (iPhone 13,
// Chromium, touch). Against a throwaway (scripts/chat-reply-check.sh): a
// swipe right on a turn puts "replying to …" over the composer; Esc takes it
// off; a swipe again and Send posts the reply — its turn draws the quoted
// header (the author, the first words) and not the token; a tap on the last
// turn's header (a reply to the chat's first message, out of the window)
// scrolls to the original and flashes it; no page errors.
//
//   node scripts/chat-reply-check.mjs ORIGIN SESSION_FILE CHAT FIRST THEME [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat, first, scheme, shots] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: scheme });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const cdp = await ctx.newCDPSession(page);
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const tag = `390-${scheme}`;
await page.goto(`${origin}/app/#/chats/${chat}`);
try {
  await page.waitForSelector("#chatfull .trow .turn", { timeout: 30000 });
} catch (e) {
  if (shots) await page.screenshot({ path: `${shots}/${tag}-no-chat.png` });
  console.log(`FAIL ${tag}: the chat never drew (${errs.join(" | ")})`);
  process.exit(1);
}
await page.waitForTimeout(1500);

// A finger: down on the turn, right in steps, up.
const swipe = async (sel, dx) => {
  const b = await page.locator(sel).boundingBox();
  const x = b.x + 30, y = b.y + Math.min(b.height / 2, 20);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
  for (let i = 1; i <= 8; i++) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: x + (dx * i) / 8, y: y + i * 0.5 }] });
    await page.waitForTimeout(16);
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(250);
};
// The turn to answer: the last one on screen that is no reply.
const target = await page.evaluate(() => {
  const rows = [...document.querySelectorAll("#chatfull .trow")].filter((r) => { const b = r.getBoundingClientRect(); return b.top > 80 && b.bottom < innerHeight - 160 && !r.querySelector(".treply"); });
  const r = rows.at(-1);
  return r ? { mid: r.getAttribute("data-mid"), text: r.querySelector(".md")?.textContent.trim() } : null;
});
ok(!!target, `${tag}: a turn to answer on screen (${target?.mid})`);
const turnSel = `#chatfull .trow[data-mid="${target?.mid}"] .turn`;
await swipe(turnSel, 40);
ok(!(await page.locator(".ccompose .creply").count()), `${tag}: a short swipe does not reply`);
await swipe(turnSel, 110);
const bar = page.locator(".ccompose .creply .crt");
ok((await bar.count()) === 1 && (await bar.textContent()).startsWith("replying to "), `${tag}: a swipe right — "${await bar.textContent().catch(() => "")}" over the composer`);
ok((await bar.textContent().catch(() => "")).includes(target.text.slice(0, 20)), `${tag}: the bar names the turn's first words`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-replying.png` });
await page.focus(".ccompose .cbox");
await page.keyboard.press("Escape");
await page.waitForTimeout(150);
ok(!(await page.locator(".ccompose .creply").count()), `${tag}: Esc takes the reply off`);
await swipe(turnSel, 110);
await page.fill(".ccompose .cbox", "swiped reply");
await page.keyboard.press("Enter");
// The new turn: mine, with the header.
let mine = null;
for (let i = 0; i < 40 && !mine; i++) {
  await page.waitForTimeout(150);
  mine = await page.evaluate(() => {
    const t = [...document.querySelectorAll("#chatfull .turn.mine")].find((t) => t.querySelector(".md")?.textContent.trim() === "swiped reply");
    return t ? { who: t.querySelector(".treply b")?.textContent, said: t.querySelector(".treply span")?.textContent, md: t.querySelector(".md").innerHTML } : null;
  });
}
ok(!!mine, `${tag}: the reply is posted and drawn`);
ok(mine && mine.who && mine.said && target.text.startsWith(mine.said.replace(/…$/, "")), `${tag}: its header — ${mine?.who}: "${mine?.said}"`);
ok(mine && !mine.md.includes("message:") && !mine.md.includes("↩"), `${tag}: its body without the token`);
ok(!(await page.locator(".ccompose .creply").count()), `${tag}: sent — the bar is gone`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-sent.png` });
// The reply to the first message: its header taps to it.
const back = page.locator("#chatfull .treply", { hasText: "archive the expired" }).last();
ok((await back.count()) === 1, `${tag}: the reply to message ${first} draws its header`);
ok(!(await page.locator(`#chatfull [data-message-id="${first}"]`).count()), `${tag}: message ${first} is out of the window before the tap`);
await back.tap();
let seen = null;
for (let i = 0; i < 40 && !seen; i++) {
  await page.waitForTimeout(150);
  seen = await page.evaluate((id) => {
    const el = document.querySelector(`#chatfull [data-message-id="${id}"]`);
    if (!el) return null;
    const b = el.getBoundingClientRect();
    return b.bottom > 60 && b.top < innerHeight - 100 && el.classList.contains("tflash") ? { top: Math.round(b.top), flash: true } : null;
  }, first);
}
ok(!!seen, `${tag}: a tap on the header scrolls to message ${first} (top ${seen?.top})`);
ok(!!seen?.flash, `${tag}: and flashes it`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-jumped.png` });
ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
await browser.close();
process.exit(fails.length ? 1 : 0);
