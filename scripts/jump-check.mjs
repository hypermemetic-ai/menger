#!/usr/bin/env node
// CHAT-62: the round ↓ docked on the composer's edge. As the reader of chat
// CHAT, at WIDTH in ENGINE (under 761 an iPhone 13, else a desktop): read
// back a few screens — the ↓ shows (.tjump.on), its 44 px hit rect meets no
// turn's text rect, it sits on the composer; one tap lands at the bottom and
// the ↓ goes. Read back again and the agent (AGENT_SESSION, on the hub at
// HUB) posts one message — the ↓ carries the badge 1. At a phone's width,
// the same with the keyboard up (scripts/kb-shim.js). scripts/jump-check.sh
// starts the throwaway and runs it.
//
//   node scripts/jump-check.mjs ORIGIN SESSION_FILE CHAT WIDTH ENGINE HUB AGENT_SESSION
import { createRequire } from "node:module";
import fs from "node:fs";
import { call, connect } from "../deploy/menger-call.mjs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat, width, engine, hub, agentFile] = process.argv.slice(2);
// One message as the agent, over the socket (no synapse in a container).
async function post(body) {
  const ws = await connect(hub, { Authorization: `Bearer ${fs.readFileSync(agentFile, "utf8").trim()}` });
  await call(ws, 1, "chats.post", { chat: +chat, body }, () => {});
  ws.close?.();
}
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${u.port}`, secure: u.protocol === "https:", value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const phone = +width < 761;
const browser = await pw[engine].launch();
const ctx = await browser.newContext(phone ? { ...pw.devices["iPhone 13"] } : { viewport: { width: +width, height: 900 } });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const consoleLines = [];
page.on("console", (m) => consoleLines.push(`${m.type()}: ${m.text()}`));
await page.goto(`${origin}/#/chats/${chat}`);
await page.waitForSelector("#chatfull .trow", { timeout: 30000 }).catch(async (e) => {
  await page.screenshot({ path: `${process.env.SHOTS ?? "/tmp"}/jump-${width}-fail.png` });
  console.log(page.url(), errs, consoleLines.slice(-10));
  throw e;
});
await page.waitForTimeout(2000);

// The scroller: the document on a phone (CHAT-29), else the list.
const scrollBy = (dy) => page.evaluate((dy) => {
  const l = document.querySelector("#chatfull .pmsgs");
  const doc = getComputedStyle(l).overflowY === "visible";
  if (doc) window.scrollBy(0, dy); else l.scrollTop += dy;
}, dy);
const off = () => page.evaluate(() => {
  const l = document.querySelector("#chatfull .pmsgs");
  if (getComputedStyle(l).overflowY === "visible") { const d = document.scrollingElement; return { off: d.scrollHeight - d.scrollTop - innerHeight, screen: innerHeight }; }
  return { off: l.scrollHeight - l.scrollTop - l.clientHeight, screen: l.clientHeight };
});
// Read back as a reader does: a few moves, each its own scroll.
async function readBack(screens) {
  const { screen } = await off();
  for (let i = 0; i < screens * 3; i++) { await scrollBy(-screen / 3); await page.waitForTimeout(120); }
  await page.waitForTimeout(500);
}
const shown = () => page.evaluate(() => {
  const b = document.querySelector(".tjump.on");
  return !!b && getComputedStyle(b).opacity === "1";
});

ok((await off()).off < 60, "opens at the bottom");
ok(!(await shown()), "at the bottom: no ↓");
await readBack(0.5);
ok(!(await shown()), "half a screen back: no ↓ yet");
await readBack(3);
const o = await off();
ok(o.off > o.screen, `a few screens back (${Math.round(o.off)} px off, a screen ${o.screen})`);
ok(await shown(), "the ↓ shows");

const geo = await page.evaluate(() => {
  const b = document.querySelector(".tjump.on").getBoundingClientRect();
  const card = document.querySelector("#chatfull .ccompose .ccard").getBoundingClientRect();
  const comp = document.querySelector("#chatfull .ccompose").getBoundingClientRect();
  // Only text a reader sees: above the composer (opaque, it hides what
  // runs under it) and inside the list's own box.
  const list = document.querySelector("#chatfull .pmsgs").getBoundingClientRect();
  const seen = { top: Math.max(0, list.top), bottom: Math.min(innerHeight, list.bottom, comp.top) };
  const hits = [];
  const walk = document.createTreeWalker(document.querySelector("#chatfull .tlist"), NodeFilter.SHOW_TEXT);
  let texts = 0;
  for (let n = walk.nextNode(); n; n = walk.nextNode()) {
    if (!n.textContent.trim()) continue;
    const r = document.createRange();
    r.selectNodeContents(n);
    for (const t of r.getClientRects()) {
      // What of the line shows: a line running under the composer is
      // hidden there (whatever sits on it).
      const top = Math.max(t.top, seen.top), bottom = Math.min(t.bottom, seen.bottom);
      if (bottom <= top || t.width === 0) continue;
      texts++;
      if (t.left < b.right && t.right > b.left && top < b.bottom && bottom > b.top) hits.push(n.textContent.slice(0, 40));
    }
  }
  const bg = getComputedStyle(document.querySelector("#chatfull .ccompose")).backgroundColor;
  return { b: [b.left, b.top, b.width, b.height].map(Math.round), card: [card.top, card.right].map(Math.round), comp: Math.round(comp.top), hits, texts, bg };
});
console.log(`     ↓ at ${geo.b} · card top/right ${geo.card} · composer top ${geo.comp} · ${geo.texts} text rects on screen`);
ok(geo.texts > 0, "turns' text is on screen");
ok(geo.hits.length === 0, `the ↓ meets no turn's text${geo.hits.length ? `: ${JSON.stringify(geo.hits.slice(0, 3))}` : ""}`);
ok(geo.b[2] >= 44 && geo.b[3] >= 44, "a 44 × 44 hit area");
ok(geo.b[1] >= geo.comp - 1, "it sits inside the composer's band (below the turns)");
ok(!/rgba\(.*, 0\)|transparent/.test(geo.bg), `the composer under it is opaque (${geo.bg})`);
ok(Math.abs(geo.b[1] + geo.b[3] - geo.card[0]) <= 6 && Math.abs(geo.b[0] + geo.b[2] - geo.card[1]) <= 6, "docked on the card's top-right edge");

if (phone) await page.tap(".tjump.on"); else await page.click(".tjump.on");
await page.waitForTimeout(1200);
ok((await off()).off < 60, `a tap lands at the bottom (${Math.round((await off()).off)} px off)`);
ok(!(await shown()), "and the ↓ goes");

await readBack(3);
await post("One more, after you read back.");
await page.waitForTimeout(2500);
const badge = await page.evaluate(() => document.querySelector(".tjump.on .tjn")?.textContent ?? null);
ok(badge === "1", `a message arriving while read back is the badge (${badge})`);
const label = await page.evaluate(() => document.querySelector(".tjump.on")?.getAttribute("aria-label"));
ok(label === "jump to latest · 1 new message", `its label says so (${label})`);
ok(errs.length === 0, `no page errors${errs.length ? `: ${errs.slice(0, 2)}` : ""}`);
await page.screenshot({ path: `${process.env.SHOTS ?? "/tmp"}/jump-${width}.png` });

// The keyboard up (scripts/kb-shim.js): the ↓ stays with the composer, above
// the keyboard; a tap lands at the bottom and keeps the keyboard and draft.
if (phone) {
  const DRAFT = "a draft to keep";
  const shim = fs.readFileSync(new URL("./kb-shim.js", import.meta.url), "utf8");
  const kctx = await browser.newContext({ ...pw.devices["iPhone 13"] });
  await kctx.addCookies([cookie]);
  await kctx.addInitScript(`window.__kbConf = { k: 336, ms: 300 };\n${shim}`);
  const k = await kctx.newPage();
  await k.goto(`${origin}/#/chats/${chat}`);
  await k.waitForSelector("#chatfull .trow", { timeout: 30000 });
  await k.waitForTimeout(2000);
  await k.evaluate(() => window.__kb.drag(1e6));
  await k.waitForTimeout(500);
  await k.tap("#chatfull textarea.cbox");
  await k.waitForTimeout(1200);
  await k.keyboard.type(DRAFT);
  const kb = () => k.evaluate(() => document.documentElement.hasAttribute("data-kb"));
  ok(await kb(), "keyboard: up");
  // Pinned to the visible area, the list scrolls itself (CHAT-29).
  for (let i = 0; i < 8; i++) { await k.evaluate(() => { document.querySelector("#chatfull .pmsgs").scrollTop -= 200; }); await k.waitForTimeout(120); }
  await k.waitForTimeout(600);
  const kg = await k.evaluate(() => {
    const b = document.querySelector(".tjump.on")?.getBoundingClientRect();
    const comp = document.querySelector("#chatfull .ccompose").getBoundingClientRect();
    return b && { b: [b.top, b.bottom].map(Math.round), comp: Math.round(comp.top), vis: Math.round(window.__kb.state().bottom) };
  });
  console.log(`     keyboard: ↓ ${kg && kg.b} · composer top ${kg?.comp} · visible to ${kg?.vis}`);
  ok(!!kg, "keyboard: the ↓ shows when read back");
  if (kg) {
    ok(kg.b[0] >= kg.comp - 1 && kg.b[1] <= kg.vis + 1, "keyboard: it sits in the composer, above the keyboard");
    await k.tap(".tjump.on");
    await k.waitForTimeout(1200);
    const left = await k.evaluate(() => { const l = document.querySelector("#chatfull .pmsgs"); return getComputedStyle(l).overflowY === "visible" ? null : l.scrollHeight - l.scrollTop - l.clientHeight; });
    ok(left !== null && left < 60, `keyboard: a tap lands at the bottom (${left} px off)`);
    ok(await kb(), "keyboard: still up");
    ok((await k.evaluate(() => document.querySelector("textarea.cbox")?.value)) === DRAFT, "keyboard: the draft is kept");
    ok(await k.evaluate(() => document.activeElement?.matches("textarea.cbox") ?? false), "keyboard: the composer keeps its focus");
  }
  await k.screenshot({ path: `${process.env.SHOTS ?? "/tmp"}/jump-${width}-kb.png` });
}
await browser.close();
console.log(`\n${fails.length === 0 ? "PASS" : `FAIL ${fails.length}`}`);
process.exit(fails.length === 0 ? 0 : 1);
