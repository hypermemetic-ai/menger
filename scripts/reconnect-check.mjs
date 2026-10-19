#!/usr/bin/env node
// APP-94: a reconnect refills the open view in place — no reload, no
// blank, the nodes on screen kept, the scroll and a typed draft where they
// were, and what changed while the socket was down drawn.
//
// Against a throwaway server (README "Commands"; scripts/throwaway-login.mjs
// makes the session file) holding a scope MAP-1 with builds MAP-2..MAP-13
// and chat 1 on MAP-2 with a screenful of messages or more:
//   node scripts/reconnect-check.mjs OUTDIR http://127.0.0.1:SHELL WSPORT SESSION_FILE
// The app's socket goes through Playwright (routeWebSocket), so the check
// cuts it and refuses new ones while it makes changes with synapse, then
// lets the app reconnect. A last case serves another build's page to the
// app's release check: that one must reload (APP-63's fallback). Exit 1 on
// a failure.
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [out, origin, wsPort, tokenFile] = process.argv.slice(2);
fs.mkdirSync(out, { recursive: true });
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
let n = 0;
const synapse = (...args) => {
  const body = args.indexOf("--body");
  if (body >= 0) {
    const f = `${out}/body-${n++}.md`;
    fs.writeFileSync(f, args[body + 1]);
    args[body + 1] = `@${f}`;
  }
  return execFileSync("synapse", ["-P", wsPort, "--token-file", tokenFile, "menger", ...args], { encoding: "utf8" });
};
const STATUSES = ["Ready", "Coding", "In Review"];

// The socket, behind a switch.
function socketSwitch(page) {
  const s = { down: false, open: [] };
  s.install = () => page.routeWebSocket(/.*/, (ws) => {
    if (s.down) return ws.close({ code: 1011, reason: "down" });
    const server = ws.connectToServer();
    s.open.push({ ws, server });
  });
  s.cut = async () => {
    s.down = true;
    for (const { ws, server } of s.open.splice(0)) {
      await ws.close({ code: 1011, reason: "cut" }).catch(() => {});
      await server.close().catch(() => {});
    }
  };
  s.restore = () => { s.down = false; };
  return s;
}

// What is on screen, sampled: the shortest the body's text got, and
// whether the page navigated.
async function watchPage(page) {
  await page.evaluate(() => {
    window.__app94 = { kept: true, minText: Infinity, removed: 0 };
    const tick = () => {
      const t = document.body.innerText.length;
      window.__app94.minText = Math.min(window.__app94.minText, t);
    };
    window.__app94.timer = setInterval(tick, 40);
    new MutationObserver((ms) => {
      for (const m of ms) for (const r of m.removedNodes) if (r.nodeType === 1) window.__app94.removed++;
    }).observe(document.body, { childList: true, subtree: true });
  });
}

async function live(page, timeout = 45_000) {
  await page.waitForFunction(() => /live/.test(document.querySelector("#conn")?.textContent || ""), null, { timeout });
}
async function reconnecting(page) {
  await page.waitForFunction(() => !/live/.test(document.querySelector("#conn")?.textContent || ""), null, { timeout: 10_000 });
}
const shot = (page, name) => page.screenshot({ path: `${out}/${name}.png` });

async function open(browser, width, hash) {
  const ctx = await browser.newContext({ viewport: { width, height: width < 500 ? 844 : 900 } });
  await ctx.addCookies([{ name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" }]);
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  const sock = socketSwitch(page);
  await sock.install();
  await page.goto(`${origin}/${hash}`);
  await live(page);
  return { ctx, page, sock, errors };
}

// The chat: rows kept, scroll kept, the draft kept and focused; a message
// posted and one deleted while away both drawn.
async function chatCase(browser, width, round) {
  const { ctx, page, sock, errors } = await open(browser, width, "#/chats/1");
  await page.waitForSelector(".trow[data-mid]");
  await page.waitForTimeout(800);
  // Up from the bottom, so the reader is not stuck to it.
  await page.evaluate(() => {
    const list = document.querySelector(".pmsgs");
    const before = list.scrollTop;
    list.scrollTop = Math.max(0, list.scrollTop - 300);
    if (list.scrollTop === before) window.scrollBy(0, -300);
  });
  await page.waitForTimeout(400);
  const draft = `half a thought ${width}`;
  await page.click("textarea.cbox");
  await page.keyboard.type(draft);
  await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".trow[data-mid]")];
    window.__held = rows;
    window.__mids = rows.map((r) => r.getAttribute("data-mid"));
    // What the reader sees: the first message row in view, and where.
    const top = Math.max(0, document.querySelector(".pmsgs").getBoundingClientRect().top);
    const first = rows.find((r) => r.getBoundingClientRect().bottom > top + 1);
    window.__scroll = { mid: first.getAttribute("data-mid"), y: first.getBoundingClientRect().top };
  });
  const held = await page.evaluate(() => Number(document.querySelector(".tlist")?.getAttribute("data-held") || 0));
  const victim = await page.evaluate(() => window.__mids[Math.floor(window.__mids.length / 2)]);
  await watchPage(page);
  await shot(page, `chat-${width}-before`);
  await sock.cut();
  await reconnecting(page);
  await shot(page, `chat-${width}-down`);
  const posted = `Posted while you were away (${width}, round ${round})`;
  synapse("chats", "post", "--chat", "1", "--body", posted);
  synapse("chats", "delete", "--chat", "1", "--ids", victim);
  sock.restore();
  await live(page);
  await page.waitForFunction((p) => [...document.querySelectorAll(".trow")].some((r) => r.textContent.includes(p)) || Number(document.querySelector(".tlist")?.getAttribute("data-held")) > 0 && document.body.textContent.includes(p), posted, { timeout: 20_000 }).catch(() => {});
  await page.waitForTimeout(600);
  const r = await page.evaluate(({ posted, victim }) => {
    const ta = document.querySelector("textarea.cbox");
    const vic = document.querySelector(`.trow[data-mid="${victim}"]`);
    return {
      kept: window.__app94?.kept === true,
      minText: window.__app94.minText,
      gone: window.__held.filter((r) => !r.isConnected).length,
      heldRows: window.__held.length,
      heldNow: Number(document.querySelector(".tlist")?.getAttribute("data-held") || 0),
      hasPosted: document.body.textContent.includes(posted),
      victimDeleted: !vic || /deleted/i.test(vic.textContent) || vic.closest(".trow")?.classList.contains("del") || !document.body.textContent.includes(`Message ${victim} —`),
      scroll: { y: document.querySelector(`.trow[data-mid="${window.__scroll.mid}"]`)?.getBoundingClientRect().top, was: window.__scroll },
      draft: ta?.value,
      focused: document.activeElement === ta,
    };
  }, { posted, victim });
  await shot(page, `chat-${width}-after`);
  ok(r.kept, `chat ${width}: the page did not reload`);
  ok(r.minText > 100, `chat ${width}: never blank (shortest text ${r.minText})`);
  ok(r.gone === 0, `chat ${width}: every message row on screen kept its node (${r.heldRows - r.gone}/${r.heldRows})`);
  ok(r.hasPosted && r.heldNow === held + 1, `chat ${width}: the message posted while away is drawn (held ${held} → ${r.heldNow})`);
  ok(r.victimDeleted, `chat ${width}: the message deleted while away is marked`);
  const moved = Math.abs((r.scroll.y ?? Infinity) - r.scroll.was.y);
  ok(moved <= 2, `chat ${width}: the scroll held — message ${r.scroll.was.mid}, the first in view, moved ${moved}px on screen`);
  ok(r.draft === draft && r.focused, `chat ${width}: the draft kept and focused (${JSON.stringify(r.draft)}, ${r.focused})`);
  ok(errors.length === 0, `chat ${width}: no page errors ${errors.join(" | ")}`);
  await ctx.close();
}

// A ticket page: its status and body changed while away are drawn; the
// nav's rows and the comment box (with its draft) kept.
async function ticketCase(browser, width, round) {
  const { ctx, page, sock, errors } = await open(browser, width, "#/ticket/MAP-3");
  await page.waitForFunction(() => document.body.textContent.includes("Body of ticket 2"), null, { timeout: 15_000 });
  const draft = `a comment, half written ${width}`;
  const box = page.locator("textarea.cbox").first();
  await box.click();
  await page.keyboard.type(draft);
  await page.evaluate(() => {
    window.__held = [...document.querySelectorAll(".row, h1, header, nav, textarea")];
  });
  await watchPage(page);
  await sock.cut();
  await reconnecting(page);
  const status = STATUSES[round % STATUSES.length];
  const amended = `Body of ticket 2, amended while away (${width}, round ${round}).`;
  synapse("tickets", "set_status", "--id", "MAP-3", "--status", status);
  synapse("tickets", "amend", "--id", "MAP-3", "--body", amended);
  sock.restore();
  await live(page);
  await page.waitForFunction((a) => document.body.textContent.includes(a), amended, { timeout: 20_000 }).catch(() => {});
  await page.waitForTimeout(600);
  const r = await page.evaluate(({ amended, status }) => {
    const ta = document.querySelector("textarea.cbox");
    return {
      kept: window.__app94?.kept === true,
      minText: window.__app94.minText,
      gone: window.__held.filter((e) => !e.isConnected).length,
      held: window.__held.length,
      body: document.body.textContent.includes(amended),
      status: [...document.querySelectorAll(".st, .status, .tag")].some((e) => e.textContent.trim() === status),
      draft: ta?.value,
      focused: document.activeElement === ta,
    };
  }, { amended, status });
  await shot(page, `ticket-${width}-after`);
  ok(r.kept, `ticket ${width}: the page did not reload`);
  ok(r.minText > 100, `ticket ${width}: never blank (shortest text ${r.minText})`);
  ok(r.gone === 0, `ticket ${width}: the nav's rows, the header and the comment box kept their nodes (${r.held - r.gone}/${r.held})`);
  ok(r.body, `ticket ${width}: the body amended while away is drawn`);
  ok(r.status, `ticket ${width}: the status set while away (${status}) is drawn`);
  ok(r.draft === draft && r.focused, `ticket ${width}: the comment draft kept and focused (${JSON.stringify(r.draft)}, ${r.focused})`);
  ok(errors.length === 0, `ticket ${width}: no page errors ${errors.join(" | ")}`);
  await ctx.close();
}

// The fallback: the shell serves another build — the page reloads.
async function releaseCase(browser, width) {
  const { ctx, page, sock } = await open(browser, width, "#/chats/1");
  await page.waitForSelector(".trow[data-mid]");
  await page.route(/\?build=/, async (route) => {
    const res = await route.fetch();
    const html = (await res.text()).replaceAll(/menger-app-[0-9a-f]+/g, "menger-app-0000000000000000");
    await route.fulfill({ response: res, body: html });
  });
  await page.evaluate(() => { window.__app94 = { kept: true }; });
  await sock.cut();
  await reconnecting(page);
  sock.restore();
  await page.waitForFunction(() => !window.__app94, null, { timeout: 45_000 }).catch(() => {});
  const reloaded = await page.evaluate(() => !window.__app94);
  ok(reloaded, `release ${width}: a new build served — the page reloaded (APP-63's fallback)`);
  await ctx.close();
}

const browser = await pw.chromium.launch();
let round = 0;
for (const width of [390, 1440]) {
  await chatCase(browser, width, round++);
  await ticketCase(browser, width, round++);
}
await releaseCase(browser, 1440);
await browser.close();
console.log(fails.length ? `\n${fails.length} failed` : "\nall passed");
process.exit(fails.length ? 1 : 0);
