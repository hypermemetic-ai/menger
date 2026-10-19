#!/usr/bin/env node
// CHAT-54: with a text box focused and the keyboard up (scripts/kb-shim.js),
// a tap does one of two things (frontend/src/tapkb.rs's table): on an action
// the action decides the keyboard (the pen keeps it, a link drops it); on
// anything else (a message's text, a header, empty space) the box blurs and
// the keyboard drops. The draft is kept; a scroll leaves the keyboard as it
// is. WebKit at 390 (iPhone 13, touch), light and dark; the full chat and a
// ticket's comment box. Against a throwaway server holding a chat CHAT whose
// newest messages hold a ticket code and a markdown link, long enough to
// scroll, and a ticket TICKET (scripts/throwaway-login.mjs makes the session):
//
//   node scripts/tap-keyboard-check.mjs http://127.0.0.1:PORT SESSION_FILE CHAT TICKET
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat, ticket] = process.argv.slice(2);
const shim = fs.readFileSync(new URL("./kb-shim.js", import.meta.url), "utf8");
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${u.port}`, secure: u.protocol === "https:", value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const DRAFT = "a draft to keep";

const browser = await pw.webkit.launch();
async function open(scheme, hash, boxSel, ready) {
  const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: scheme });
  await ctx.addCookies([cookie]);
  await ctx.addInitScript(`window.__kbConf = { k: 336, ms: 300 };\n${shim}`);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  await page.goto(`${origin}/#/${hash}`);
  await page.waitForSelector(ready, { timeout: 30000 });
  await page.waitForTimeout(2000);
  if (hash.startsWith("chats")) { await page.evaluate(() => window.__kb.drag(1e6)); await page.waitForTimeout(500); }
  await page.tap(boxSel);
  await page.waitForTimeout(1200);
  await page.keyboard.type(DRAFT);
  await page.waitForTimeout(300);
  return { ctx, page, errs, boxSel };
}
const state = (s) => s.page.evaluate((sel) => ({
  focused: document.activeElement?.matches?.(sel) ?? false,
  kb: document.documentElement.hasAttribute("data-kb"),
  value: document.querySelector(sel)?.value ?? null,
  sel: String(getSelection()),
  held: !!document.querySelector(".turn.held"),
}), s.boxSel);
const up = (st) => st.focused && st.kb;
const down = (st) => !st.focused && !st.kb;

for (const scheme of ["light", "dark"]) {
  const t = `${scheme}:`;
  const chatBox = "#chatfull textarea.cbox";
  // ── the chat ──
  // anything else: a message's text, the header, empty space
  for (const [what, sel] of [["a message's text", "#chatfull .turn .tbody, #chatfull .turn p"], ["the header", "#chatfull .chead, #chatfull header"], ["the list's background", "#chatfull .pmsgs"]]) {
    const s = await open(scheme, `chats/${chat}`, chatBox, "#chatfull .trow");
    ok(up(await state(s)), `${t} ${what}: keyboard up before the tap`);
    const h = await s.page.$(sel.split(", ").map((x) => x).join(", "));
    if (!h) { ok(false, `${t} ${what}: no target found (${sel})`); await s.ctx.close(); continue; }
    if (what === "the list's background") {
      const b = await h.boundingBox();
      await s.page.touchscreen.tap(b.x + 3, b.y + 3);
    } else await h.tap({ force: true });
    await s.page.waitForTimeout(900);
    const st = await state(s);
    ok(down(st), `${t} ${what}: one tap drops the keyboard (focused ${st.focused}, kb ${st.kb})`);
    ok(st.value === DRAFT, `${t} ${what}: the draft is kept (${st.value})`);
    ok(st.sel === "" && !st.held, `${t} ${what}: no selection, no menu`);
    ok(s.errs.length === 0, `${t} ${what}: no page errors ${s.errs.join(" | ")}`);
    await s.ctx.close();
  }
  // an action that keeps it: the pen
  {
    const s = await open(scheme, `chats/${chat}`, chatBox, "#chatfull .trow");
    await s.page.tap("#chatfull .cdraw");
    await s.page.waitForTimeout(900);
    const st = await state(s);
    ok(up(st), `${t} the pen: the keyboard stays up (focused ${st.focused}, kb ${st.kb})`);
    ok(st.value === DRAFT, `${t} the pen: the draft is kept`);
    await s.ctx.close();
  }
  // an action that keeps it: Send (the draft goes, the box keeps the keyboard)
  {
    const s = await open(scheme, `chats/${chat}`, chatBox, "#chatfull .trow");
    await s.page.tap("#chatfull .csend");
    await s.page.waitForTimeout(1200);
    const st = await state(s);
    ok(up(st), `${t} Send: the keyboard stays up (focused ${st.focused}, kb ${st.kb})`);
    await s.ctx.close();
  }
  // an action that drops it: a link whose tap stays in the page
  {
    const s = await open(scheme, `chats/${chat}`, chatBox, "#chatfull .trow");
    await s.page.evaluate(() => { const a = [...document.querySelectorAll('#chatfull .pmsgs a:not(.tchip)')].at(-2); a.id = "thelink"; a.setAttribute("href", "#/chats/" + location.hash.split("/").pop()); });
    await s.page.tap("#thelink");
    await s.page.waitForTimeout(900);
    const st = await state(s);
    ok(down(st), `${t} a link: one tap runs it and drops the keyboard (focused ${st.focused}, kb ${st.kb})`);
    ok(st.value === DRAFT, `${t} a link: the draft is kept (${st.value})`);
    await s.ctx.close();
  }
  // an action that drops it: a chip opens the ticket, the keyboard goes
  {
    const s = await open(scheme, `chats/${chat}`, chatBox, "#chatfull .trow");
    await s.page.tap("#chatfull a.tchip >> nth=-1");
    await s.page.waitForTimeout(1200);
    const hash = await s.page.evaluate(() => location.hash);
    const st = await s.page.evaluate(() => ({ kb: document.documentElement.hasAttribute("data-kb"), field: document.activeElement?.matches?.("textarea, input") ?? false }));
    ok(/^#\/ticket\//.test(hash) && !st.kb && !st.field, `${t} a chip: one tap opens ${hash} and the keyboard is down (kb ${st.kb}, field ${st.field})`);
    await s.ctx.close();
  }
  // a scroll is not a tap
  {
    const s = await open(scheme, `chats/${chat}`, chatBox, "#chatfull .trow");
    await s.page.evaluate(() => window.__kb.drag(-400));
    await s.page.waitForTimeout(700);
    const st = await state(s);
    ok(up(st) && st.value === DRAFT, `${t} a scroll: the keyboard stays up, the draft kept (focused ${st.focused}, kb ${st.kb})`);
    await s.ctx.close();
  }
  // ── a ticket's comment box ──
  {
    const box = "#conversation textarea";
    const s = await open(scheme, `ticket/${ticket}`, box, "#conversation textarea");
    ok(up(await state(s)), `${t} comment: keyboard up before the tap`);
    await s.page.touchscreen.tap(8, 70);
    await s.page.waitForTimeout(900);
    const st = await state(s);
    ok(down(st), `${t} comment: a tap elsewhere drops the keyboard (focused ${st.focused}, kb ${st.kb})`);
    ok(st.value === DRAFT, `${t} comment: the draft is kept (${st.value})`);
    ok(s.errs.length === 0, `${t} comment: no page errors ${s.errs.join(" | ")}`);
    await s.ctx.close();
  }
}
await browser.close();
console.log(`\n${fails.length === 0 ? "PASS" : `FAIL ${fails.length}`}`);
process.exit(fails.length === 0 ? 0 : 1);
