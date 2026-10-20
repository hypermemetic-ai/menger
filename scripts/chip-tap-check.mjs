#!/usr/bin/env node
// CHAT-50: with the composer focused and the keyboard up (scripts/kb-shim.js),
// ONE tap on a ticket chip, a link or "jump to latest" in the chat does what
// it says, and the draft is kept. WebKit at 390 (iPhone 13). Against a
// throwaway server holding a chat CHAT whose newest messages hold a ticket
// code and a markdown link, long enough to scroll; scripts/throwaway-login.mjs makes
// the session file.
//
//   node scripts/chip-tap-check.mjs http://127.0.0.1:PORT SESSION_FILE CHAT
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat] = process.argv.slice(2);
const shim = fs.readFileSync(new URL("./kb-shim.js", import.meta.url), "utf8");
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${u.port}`, secure: u.protocol === "https:", value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const DRAFT = "a draft to keep";

const browser = await pw.webkit.launch();
async function open(back = 0) {
  const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: "dark" });
  await ctx.addCookies([cookie]);
  await ctx.addInitScript(`window.__kbConf = { k: 336, ms: 300 };\n${shim}`);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  // What the page sees of a tap: its mousedown (prevented = the box keeps
  // its focus, as Send's does) and the click (is the box still focused,
  // the keyboard still up). External links are held back.
  await page.addInitScript(() => {
    window.__taps = [];
    const box = () => document.activeElement?.matches?.("textarea.cbox") ?? false;
    window.addEventListener("mousedown", (e) => window.__taps.push({ type: "mousedown", prevented: e.defaultPrevented }));
    window.addEventListener("click", (e) => {
      const a = e.target.closest && e.target.closest("a");
      window.__taps.push({ type: "click", on: String(e.target.className || e.target.tagName), focused: box(), kb: document.documentElement.hasAttribute("data-kb") });
      if (a && /^https?:/.test(a.getAttribute("href") || "")) e.preventDefault();
    });
  });
  await page.goto(`${origin}${process.env.APP_PATH ?? "/"}#/chats/${chat}`);
  await page.waitForSelector("#chatfull .trow", { timeout: 30000 });
  await page.waitForTimeout(2000);
  await page.evaluate(() => window.__kb.drag(1e6));
  await page.waitForTimeout(500);
  if (back) {
    await page.evaluate((y) => window.__kb.drag(-y), back);
    await page.waitForTimeout(500);
  }
  await page.tap("#chatfull textarea.cbox");
  await page.waitForTimeout(1200);
  await page.keyboard.type(DRAFT);
  await page.waitForTimeout(300);
  await page.evaluate(() => { window.__taps = []; });
  return { ctx, page, errs };
}
const draft = (page) => page.evaluate(() => document.querySelector("textarea.cbox")?.value ?? null);
const focused = (page) => page.evaluate(() => document.activeElement?.matches("textarea.cbox") ?? false);

// 1. a chip
{
  const { ctx, page, errs } = await open();
  ok(await page.evaluate(() => document.documentElement.hasAttribute("data-kb")), "chip: keyboard up before the tap");
  await page.tap("#chatfull a.tchip >> nth=-1");
  await page.waitForTimeout(1000);
  const h = await page.evaluate(() => location.hash);
  ok(/^#\/ticket\/KILN-\d$/.test(h), `chip: one tap opens the ticket (${h})`);
  const t = await page.evaluate(() => window.__taps);
  ok(t.find((e) => e.type === "mousedown")?.prevented === true, "chip: the tap's mousedown is prevented (the box keeps its focus)");
  ok(t.find((e) => e.type === "click")?.focused === true, "chip: the box still has focus when the click lands");
  await ctx.close();
  ok(errs.length === 0, `chip: no page errors ${errs.join(" | ")}`);
}
// 2. a link
{
  const { ctx, page } = await open();
  // A link whose tap stays in the page: WebKit itself drops the focus for a
  // tap on a link that leaves it, whatever the page does with mousedown.
  await page.evaluate(() => { const a = [...document.querySelectorAll('#chatfull .pmsgs a:not(.tchip)')].at(-2); a.id = "thelink"; a.setAttribute("href", "#/chats/" + location.hash.split("/").pop()); });
  await page.tap("#thelink");
  await page.waitForTimeout(600);
  const c = await page.evaluate(() => window.__taps);
  ok(c.filter((e) => e.type === "click").length === 1 && c.find((e) => e.type === "mousedown")?.prevented === true, `link: one tap reaches the link's click, its mousedown prevented (${JSON.stringify(c)})`);
  // CHAT-54: a link is an action that drops the keyboard once it has run.
  ok(!(await page.evaluate(() => document.documentElement.hasAttribute("data-kb"))), "link: the keyboard is down (CHAT-54's table)");
  ok((await draft(page)) === DRAFT, `link: the draft is kept (${await draft(page)})`);
  ok(!(await focused(page)), "link: the composer is blurred after the click");
  await ctx.close();
}
// 3. jump to latest
{
  const { ctx, page } = await open(1500);
  // CHAT-62: the ↓ is always in the composer; it shows (.on) a screen off.
  const has = await page.evaluate(() => !!document.querySelector(".tjump.on"));
  ok(has, "jump: the ↓ shows when read back");
  if (has) {
    const where = () => page.evaluate(() => window.__kb.state().realY + (document.querySelector(".pmsgs")?.scrollTop ?? 0));
    const y0 = await where();
    await page.tap(".tjump.on");
    await page.waitForTimeout(1500);
    const y1 = await where();
    const gone = await page.evaluate(() => !document.querySelector(".tjump.on"));
    ok(gone && y1 > y0, `jump: one tap goes to the latest (scrollY ${Math.round(y0)} → ${Math.round(y1)}, ↓ gone ${gone})`);
    ok((await draft(page)) === DRAFT, "jump: the draft is kept");
    ok(await focused(page), "jump: the composer keeps its focus");
    ok(await page.evaluate(() => document.documentElement.hasAttribute("data-kb")), "jump: the keyboard is still up");
  }
  await ctx.close();
}
await browser.close();
console.log(`\n${fails.length === 0 ? "PASS" : `FAIL ${fails.length}`}`);
process.exit(fails.length === 0 ? 0 : 1);
