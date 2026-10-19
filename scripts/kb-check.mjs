#!/usr/bin/env node
// CHAT-42: the keyboard cases, on WebKit at 390 with scripts/kb-shim.js's
// iPhone keyboard. Each case focuses a field and asserts that what is typed
// in is on screen: the field inside the visible area (the true one, above
// the keyboard), and for a pinned chat its head at the visible area's top
// and its composer on the keyboard. Run it on any change to
// frontend/src/keyboard.rs, #chatfull's rules or the composer's CSS.
//
// Against a throwaway server holding: a direct chat CHAT (#/chats/CHAT),
// a ticket TICKET with a chat of its own (TCHAT) in its dock and an
// `<m-input kind="text">` in its body; both chats long enough to scroll.
// scripts/throwaway-login.mjs makes the session file.
//
//   node scripts/kb-check.mjs OUTDIR http://127.0.0.1:PORT SESSION_FILE TICKET CHAT TCHAT
//
// WebKit runs in the Playwright image (docker, --network host). Exit 1 on a
// failure; OUTDIR gets a shot per case (the visible area outlined, the
// keyboard drawn).
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [out, origin, tokenFile, ticket, chat, tchat] = process.argv.slice(2);
fs.mkdirSync(out, { recursive: true });
const shim = fs.readFileSync(new URL("./kb-shim.js", import.meta.url), "utf8");
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${u.port}`, secure: u.protocol === "https:", value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const f0 = (n) => (n == null ? "∅" : Math.round(n));

const browser = await pw.webkit.launch();
async function phone(k = 336) {
  const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: "dark" });
  await ctx.addCookies([cookie]);
  await ctx.addInitScript(`window.__kbConf = { k: ${k}, ms: 300 };\n${shim}`);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  return { ctx, page, errs };
}

// The rects of `sels` (first match each, through frames for `frame:`), and
// the shim's state.
const look = (page, sels) => page.evaluate((sels) => {
  const r = (s) => {
    let doc = document, dy = 0;
    if (s.startsWith("frame:")) {
      const f = document.querySelector("iframe.framed, .framed iframe, iframe");
      if (!f) return null;
      dy = f.getBoundingClientRect().top;
      doc = f.contentDocument;
      s = s.slice(6);
    }
    const e = doc && doc.querySelector(s);
    if (!e) return null;
    const b = e.getBoundingClientRect();
    if (b.width === 0 && b.height === 0) return null;
    return { top: b.top + dy, bottom: b.bottom + dy };
  };
  const o = {};
  for (const s of sels) o[s] = r(s);
  return { ...o, kb: window.__kb.state(), dataKb: document.documentElement.hasAttribute("data-kb") };
}, sels);

// The phone's screen: the layout viewport, the true visible area outlined
// and the keyboard drawn under it.
async function shot(page, name) {
  const s = await page.evaluate(() => window.__kb.state());
  await page.evaluate((s) => {
    for (const e of document.querySelectorAll(".kbshim-draw")) e.remove();
    const vis = document.createElement("div");
    vis.className = "kbshim-draw";
    vis.style.cssText = `position:fixed;left:0;right:0;top:${s.top}px;height:${s.bottom - s.top}px;outline:3px solid #f0f;outline-offset:-3px;pointer-events:none;z-index:2147483646`;
    const kb = document.createElement("div");
    kb.className = "kbshim-draw";
    kb.style.cssText = `position:fixed;left:0;right:0;top:${s.bottom}px;height:${s.kb}px;background:#3a3a3c;color:#fff;font:14px sans-serif;display:grid;place-items:center;pointer-events:none;z-index:2147483647`;
    kb.textContent = `keyboard · visible area ${Math.round(s.top)}–${Math.round(s.bottom)}`;
    document.documentElement.append(vis, kb);
  }, s);
  await page.screenshot({ path: `${out}/${name}.png` });
  await page.evaluate(() => { for (const e of document.querySelectorAll(".kbshim-draw")) e.remove(); });
}

// The row the reader is at: the first under the full chat's head, and its top.
const readingAt = (page) => page.evaluate(() => {
  const head = document.querySelector("#chatfull .shead")?.getBoundingClientRect().bottom ?? 0;
  const row = [...document.querySelectorAll("#chatfull .trow")].find((r) => r.getBoundingClientRect().bottom > head + 4);
  return row ? { mid: row.getAttribute("data-mid"), top: row.getBoundingClientRect().top } : null;
});
const rowTop = (page, mid) => page.evaluate((mid) => document.querySelector(`#chatfull .trow[data-mid="${mid}"]`)?.getBoundingClientRect().top ?? null, mid);
const atEnd = (page) => page.evaluate(() => document.scrollingElement.scrollHeight - window.__kb.state().realY - document.documentElement.clientHeight);

const inside = (r, s, slack = 1) => r && r.top >= s.top - slack && r.bottom <= s.bottom + slack;

// A pinned chat with the keyboard up: head at the top, composer on the
// keyboard, both inside what the reader sees.
function pinned(tag, v, root) {
  const s = v.kb;
  const head = v[`${root} .shead`] ?? v[`${root} .dchead`];
  const comp = v[`${root} .ccompose`];
  ok(v.dataKb, `${tag}: data-kb is set`);
  ok(inside(comp, s), `${tag}: the composer is on screen (${f0(comp?.top)}–${f0(comp?.bottom)} in ${f0(s.top)}–${f0(s.bottom)})`);
  ok(comp && Math.abs(comp.bottom - s.bottom) <= 12, `${tag}: the composer sits on the keyboard (bottom ${f0(comp?.bottom)}, keyboard ${f0(s.bottom)})`);
  ok(inside(head, s) && head.top - s.top <= 12, `${tag}: the head is at the top of what is seen (${f0(head?.top)}, visible from ${f0(s.top)})`);
}

// A tap on the field: iOS raises the keyboard for a focus a finger made.
async function focus(page, sel) {
  await page.tap(sel);
  await page.waitForTimeout(1200);
}
async function blur(page) {
  await page.evaluate(() => { let a = document.activeElement; while (a && a.tagName === "IFRAME" && !(a.hasAttribute("sandbox") && !/\ballow-same-origin\b/.test(a.getAttribute("sandbox"))) && a.contentDocument) a = a.contentDocument.activeElement; a?.blur(); document.activeElement?.blur(); });
  await page.waitForTimeout(1200);
}

for (const k of [336, 374]) {
  const kt = `k${k}`;
  // ── 1. The full chat (CHAT-29: the document scrolls it), read at its end ──
  {
    const { ctx, page, errs } = await phone(k);
    await page.goto(`${origin}/#/chats/${chat}`);
    await page.waitForSelector("#chatfull .trow", { timeout: 30000 });
    await page.waitForTimeout(2000);
    await page.evaluate(() => window.__kb.drag(1e6));
    await page.waitForTimeout(500);
    const y0 = (await look(page, [])).kb.realY;
    const tag = `${kt} full chat, at the end`;
    ok(y0 > 200, `${tag}: the document scrolls the thread (scrollY ${f0(y0)})`);
    await focus(page, "#chatfull textarea.cbox");
    const v = await look(page, ["#chatfull .shead", "#chatfull .ccompose", "#chatfull .tlist > :last-child"]);
    pinned(tag, v, "#chatfull");
    const last = v["#chatfull .tlist > :last-child"];
    ok(last && last.bottom <= v["#chatfull .ccompose"]?.top + 1 && last.bottom > v.kb.top, `${tag}: the newest message is right above the composer (${f0(last?.bottom)})`);
    await shot(page, `${kt}-1-full-end`);
    await blur(page);
    const b = await look(page, ["#chatfull .ccompose"]);
    const off = await atEnd(page);
    ok(!b.dataKb && off <= 48, `${tag}: keyboard down, still at the end (${f0(off)} px off)`);
    ok(inside(b["#chatfull .ccompose"], b.kb), `${tag}: keyboard down, the composer on screen`);
    ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
    await ctx.close();
  }
  // ── 2. The full chat, read back up the thread ──
  {
    const { ctx, page, errs } = await phone(k);
    await page.goto(`${origin}/#/chats/${chat}`);
    await page.waitForSelector("#chatfull .trow", { timeout: 30000 });
    await page.waitForTimeout(2000);
    await page.evaluate(() => window.__kb.drag(1e6));
    await page.waitForTimeout(300);
    await page.evaluate(() => window.__kb.drag(-1500));
    await page.waitForTimeout(500);
    const r0 = await readingAt(page);
    const tag = `${kt} full chat, read back`;
    await focus(page, "#chatfull textarea.cbox");
    const v = await look(page, ["#chatfull .shead", "#chatfull .ccompose"]);
    pinned(tag, v, "#chatfull");
    const r1 = r0 && (await rowTop(page, r0.mid));
    ok(r1 != null && r1 >= v.kb.top - 1 && r1 < v["#chatfull .ccompose"].top, `${tag}: the row being read shows in the pinned thread (${f0(r1)})`);
    await shot(page, `${kt}-2-full-back`);
    await blur(page);
    const b = await look(page, ["#chatfull .ccompose"]);
    const r2 = r0 && (await rowTop(page, r0.mid));
    ok(!b.dataKb && r2 != null && Math.abs(r2 - r0.top) <= 48, `${tag}: keyboard down, back where you were (row ${r0?.mid} at ${f0(r0?.top)} → ${f0(r2)})`);
    ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
    await ctx.close();
  }
  // ── 3. The dock's chat on a ticket (APP-56/58/62) ──
  {
    const { ctx, page, errs } = await phone(k);
    await page.goto(`${origin}/#/ticket/${ticket}`);
    await page.waitForSelector('#pbar button[data-place="sections"]', { timeout: 30000 });
    await page.waitForTimeout(1500);
    await page.evaluate(() => window.__kb.drag(140));
    await page.waitForTimeout(300);
    const yPage = (await look(page, [])).kb.realY;
    await page.tap('#pbar button[data-place="sections"]');
    await page.waitForTimeout(500);
    await page.tap(`#tdock-sheet button.in.chat[data-chat="${tchat}"]`);
    await page.waitForSelector("#tdock-sheet .ccompose textarea.cbox", { timeout: 15000 });
    await page.waitForTimeout(800);
    const y0 = (await look(page, [])).kb.realY;
    const tag = `${kt} dock's chat`;
    await focus(page, "#tdock-sheet textarea.cbox");
    const v = await look(page, ["#tdock-sheet .ccompose", "#tdock-sheet .dchead", "#pbar"]);
    const comp = v["#tdock-sheet .ccompose"];
    ok(v.dataKb, `${tag}: data-kb is set`);
    ok(inside(comp, v.kb), `${tag}: the composer is on screen (${f0(comp?.top)}–${f0(comp?.bottom)} in ${f0(v.kb.top)}–${f0(v.kb.bottom)})`);
    ok(comp && Math.abs(comp.bottom - v.kb.bottom) <= 12, `${tag}: the composer sits on the keyboard (${f0(comp?.bottom)} / ${f0(v.kb.bottom)})`);
    ok(inside(v["#tdock-sheet .dchead"], v.kb), `${tag}: its head is on screen`);
    ok(!v["#pbar"], `${tag}: the nav is hidden`);
    ok(Math.abs(v.kb.realY - y0) <= 2, `${tag}: the page under it is held (scrollY ${f0(y0)} → ${f0(v.kb.realY)}) (APP-58)`);
    await shot(page, `${kt}-3-dock-chat`);
    await blur(page);
    const b = await look(page, ["#pbar"]);
    ok(!b.dataKb && b["#pbar"], `${tag}: keyboard down, the nav back (data-kb ${b.dataKb}, nav ${!!b["#pbar"]})`);
    await page.tap("#tdock-scrim").catch(() => {});
    await page.waitForTimeout(800);
    const c = await look(page, []);
    ok(Math.abs(c.kb.realY - yPage) <= 2, `${tag}: the sheet closed, the page where it was (scrollY ${f0(yPage)} → ${f0(c.kb.realY)})`);
    ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
    await ctx.close();
  }
  // ── 4. A ticket's comment box, and 5. a body's text input (the page's own fields: iOS reveals them) ──
  {
    const { ctx, page, errs } = await phone(k);
    await page.goto(`${origin}/#/ticket/${ticket}`);
    await page.waitForSelector("#conversation textarea", { timeout: 30000 });
    await page.waitForTimeout(2500);
    const box = await page.$("#conversation textarea");
    const tag4 = `${kt} a ticket's comment box`;
    if (box) {
      await box.scrollIntoViewIfNeeded();
      await page.evaluate(() => window.__kb.drag(0));
      await box.tap();
      await page.waitForTimeout(1200);
      const r = await box.boundingBox();
      const s = (await look(page, [])).kb;
      ok(r && r.y >= s.top - 1 && r.y + Math.min(r.height, 40) <= s.bottom + 1, `${tag4}: the box is on screen (${f0(r?.y)} in ${f0(s.top)}–${f0(s.bottom)})`);
      await shot(page, `${kt}-4-comment`);
      await blur(page);
    } else ok(false, `${tag4}: no comment box found`);
    const tag5 = `${kt} a body's text input`;
    const frame = page.frames().find((f) => f !== page.mainFrame());
    const field = frame && (await frame.$("m-input textarea, m-input input, textarea, input[type=text]"));
    if (field) {
      await field.scrollIntoViewIfNeeded();
      await field.tap();
      // APP-83: the frame is sandboxed — the shim cannot see its field, iOS
      // can: tell it, as the keyboard would come.
      await page.evaluate((r) => window.__kb.frameField(r), await field.boundingBox());
      await page.waitForTimeout(1200);
      const fr = await field.boundingBox();
      const v = await look(page, ["#pbar"]);
      const s = v.kb;
      ok(fr && fr.y >= s.top - 1 && fr.y + Math.min(fr.height, 40) <= s.bottom + 1, `${tag5}: the field is on screen (${f0(fr?.y)} in ${f0(s.top)}–${f0(s.bottom)})`);
      ok(v.dataKb && !v["#pbar"], `${tag5}: the app knows the keyboard is up, the nav hidden (data-kb ${v.dataKb}, nav ${!!v["#pbar"]}) — APP-83`);
      await shot(page, `${kt}-5-body-input`);
      await field.evaluate((el) => el.blur());
      await page.evaluate(() => window.__kb.frameField(null));
      await blur(page);
      await page.waitForTimeout(900);
      const w = await look(page, ["#pbar"]);
      ok(!w.dataKb && w["#pbar"], `${tag5}: the field left, the nav back (data-kb ${w.dataKb}, nav ${!!w["#pbar"]})`);
    } else ok(false, `${tag5}: no field in the body's frame (seed the house components: m-house, m-input)`);
    ok(errs.length === 0, `${kt} ticket page: no page errors ${errs.join(" | ")}`);
    await ctx.close();
  }
}

// ── 1440: no keyboard, nothing moves ──
{
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  await page.goto(`${origin}/#/chats/${chat}`);
  await page.waitForSelector("#chatfull textarea.cbox", { timeout: 30000 });
  await page.waitForTimeout(1500);
  const before = await page.evaluate(() => document.querySelector("#chatfull .ccompose").getBoundingClientRect().bottom);
  await page.focus("#chatfull textarea.cbox");
  await page.waitForTimeout(600);
  const v = await page.evaluate(() => ({ kb: document.documentElement.hasAttribute("data-kb"), b: document.querySelector("#chatfull .ccompose").getBoundingClientRect().bottom }));
  ok(!v.kb && Math.abs(v.b - before) <= 1, `1440: focusing the composer changes nothing (data-kb ${v.kb}, bottom ${f0(before)} → ${f0(v.b)})`);
  await ctx.close();
}

await browser.close();
console.log(`\n${fails.length === 0 ? "PASS" : `FAIL ${fails.length}`}`);
process.exit(fails.length === 0 ? 0 : 1);
