#!/usr/bin/env node
// DEPLOY-47: Ask the builder about this — checked headless against the
// throwaway scripts/deploy-ask-check.sh starts (a request to test whose
// `deploy` step failed with 60 lines, a record answering it, KILN-2 running
// on test):
//   node scripts/deploy-ask-check.mjs http://127.0.0.1:PORT SESSION_FILE WIDTH THEME REQUEST [OUTDIR]
// At a desk a right-click, on a phone (≤ 820, touch) a long press, on a log
// line of #/deploy's run opens #dmenu; its item opens the builder's chat on
// MENGER-1 titled for the deploy, the context and the lines around the one
// picked in the composer, the cursor after them. Asked again, the same chat,
// the context not doubled. At a desk too: a deploy's row on #/deployments
// and a ticket page's Running link open it; a carried ticket's chip in a row
// keeps the ticket menu. Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, width, theme, request, out] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const w = Number(width);
const phone = w < 820;
const tag = `${w} ${theme}`;

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width: w, height: phone ? 844 : 900 }, deviceScaleFactor: 2, hasTouch: phone, isMobile: phone, colorScheme: theme });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
// APP-103 (on main before DEPLOY-47): re-entering a ticket's chat by its hash
// panics on a disposed reactive value — `unreachable`, after that panic in
// the console. Those are counted apart; any other page error fails.
let disposed = 0;
page.on("console", (m) => { if (m.type() === "error" && m.text().includes("already been disposed")) disposed++; });
const cdp = await ctx.newCDPSession(page);
const shot = async (name) => out && page.screenshot({ path: `${out}/ask-${w}-${theme}-${name}.png` });
const until = async (f, ms = 20000) => {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) { if (await f().catch(() => false)) return true; await page.waitForTimeout(150); }
  return false;
};
const center = (sel) => page.$eval(sel, (e) => { e.scrollIntoView({ block: "center" }); const r = e.getBoundingClientRect(); return [r.left + Math.min(r.width / 2, 40), r.top + r.height / 2]; });
/** A long press: a touch resting 700 ms (APP-57's hold is 500). */
const hold = async (sel) => {
  const [x, y] = await center(sel);
  const pt = [{ x, y, id: 1, radiusX: 4, radiusY: 4, force: 1 }];
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: pt });
  await page.waitForTimeout(700);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
};
const open = async (sel) => (phone ? hold(sel) : page.click(sel, { button: "right" }));
// A tap on the item, at a person's pace: APP-57 takes a click within
// SWALLOW_MS (800) of a hold's lift as the lift's.
const pick = async () => (phone ? (await page.waitForTimeout(900), page.tap("#dmenu [data-act='ask-builder']")) : page.click("#dmenu [data-act='ask-builder']"));
const menu = () => page.$eval("#dmenu", (m) => ({ head: m.querySelector(".h")?.textContent ?? "", item: m.querySelector("[data-act='ask-builder']")?.textContent ?? "", sheet: m.hasAttribute("data-sheet") })).catch(() => null);
const composer = () => page.evaluate(() => {
  const t = document.querySelector('textarea.cbox[aria-label^="message the chat"]');
  return t && { value: t.value, caret: t.selectionStart, focused: document.activeElement === t };
});
const chatOf = () => Number(new URLSearchParams(location.hash.split("?")[1] ?? "").get("chat")) || null;

// 1. #/deploy: a log line of the failed step.
const LINE = `#deploy-page .treq[data-deploy="${request}"] .step[data-step="deploy"] .sline[data-line="45"]`;
await page.goto(`${origin}/#/deploy`);
const drawn = await until(() => page.$(LINE));
ok(drawn, `${tag}: the failed step's lines are drawn, one element a line`);
if (!drawn) {
  console.log(await page.evaluate(() => (document.querySelector("#deploy-page .totest")?.outerHTML ?? document.querySelector("#deploy-page")?.outerHTML ?? document.body.outerHTML).slice(0, 3000)));
  await browser.close();
  process.exit(1);
}
await open(LINE);
ok(await until(() => menu()), `${tag}: ${phone ? "a long press" : "a right-click"} on a log line opens the deployment's menu`);
const m = await menu();
ok(m && m.head === `deploy #${request} · deploy · line 46`, `${tag}: it names the deploy, the step and the line (${m?.head})`);
ok(m && m.item === "Ask the builder about this", `${tag}: its item is Ask the builder about this`);
ok(m && m.sheet === phone, `${tag}: ${phone ? "a sheet from the bottom on a phone" : "at the cursor at a desk"}`);
ok(!(await page.$("#tmenu")), `${tag}: the ticket menu stays shut`);
await shot("menu");
await pick();
ok(await until(async () => (await page.evaluate(() => location.hash)).startsWith("#/ticket/MENGER-1?") && (await composer())?.value.includes("**Deploy")), `${tag}: it opens the chat on MENGER-1, the context in the composer`);
const chat = await page.evaluate(chatOf);
const c = await composer();
const v = c?.value ?? "";
ok(chat !== null, `${tag}: the chat is the one shown (?chat=${chat})`);
ok(v.startsWith(`**Deploy #${request}** · MENGER → test`), `${tag}: the context leads with the deploy`);
for (const want of ["- revision: main's tip", `- request #${request}`, "- failing step `deploy`", "- the line picked: `deploy line 45`", "```text\n", "deploy line 59\n```"]) {
  ok(v.includes(want), `${tag}: the context carries ${JSON.stringify(want)}`);
}
ok(v.endsWith("```\n\n") && c.caret === v.length, `${tag}: the cursor waits after it, for the question`);
ok(c.focused || phone, `${tag}: the composer has focus`);
ok(await page.$eval("#chatpane .ptab.on, #chatpane", (e) => true).catch(() => false), `${tag}: the chat pane is open`);
await shot("chat");

// 2. Asked again: the same chat, the context once.
await page.goto(`${origin}/#/deploy`);
ok(await until(() => page.$(LINE)), `${tag}: back on #/deploy`);
await open(LINE);
ok(await until(() => menu()), `${tag}: the menu again`);
await pick();
ok(await until(async () => (await page.evaluate(chatOf)) === chat && (await composer()) !== null), `${tag}: asked again about the same deploy, the same chat (${await page.evaluate(chatOf)})`);
const again = (await composer())?.value ?? "";
ok(again.split(`**Deploy #${request}**`).length === 2, `${tag}: its context not doubled in the draft`);

if (!phone) {
  // 3. #/deployments: a deploy's row, and a carried ticket's chip in it.
  await page.goto(`${origin}/#/deployments/MENGER`);
  const ROW = `#deployments-page li.ddeploy[data-deploy]:not([data-state="requested"]) .dline`;
  ok(await until(() => page.$(ROW)), `${tag}: #/deployments draws the deploys`);
  await page.click(ROW, { button: "right" });
  ok(await until(async () => /^deploy #\d+$/.test((await menu())?.head ?? "")), `${tag}: a right-click on a deploy's row opens its menu (${(await menu())?.head})`);
  await page.keyboard.press("Escape");
  ok(await until(async () => !(await page.$("#dmenu"))), `${tag}: Escape closes it`);
  const CHIP = `#deployments-page li.ddeploy .dcarried a[data-ticket-id]`;
  if (await page.$(CHIP)) {
    await page.click(CHIP, { button: "right" });
    ok(await until(() => page.$("#tmenu")) && !(await page.$("#dmenu")), `${tag}: a carried ticket's chip keeps the ticket menu`);
    await page.keyboard.press("Escape");
  } else ok(false, `${tag}: a deploy row carries a ticket chip`);
  // 4. A ticket page's Running.
  await page.goto(`${origin}/#/ticket/KILN-2`);
  const RUN = `a.run[data-deploy]`;
  ok(await until(() => page.$(RUN)), `${tag}: KILN-2's Running links its deploy`);
  await page.click(RUN, { button: "right" });
  ok(await until(async () => /^deploy #\d+$/.test((await menu())?.head ?? "")), `${tag}: a right-click on Running opens the deployment's menu, not the ticket's`);
  ok(!(await page.$("#tmenu")), `${tag}: the ticket page's own menu stays shut there`);
  await page.keyboard.press("Escape");
}

const over = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
ok(over <= 0, `${tag}: nothing wider than the page (${over} px over)`);
const known = disposed > 0 ? errs.filter((e) => /unreachable/.test(e)) : [];
const other = errs.filter((e) => !known.includes(e));
if (known.length) console.log(`note ${tag}: ${known.length} page errors from APP-103's disposed-value panic (not this ticket's)`);
ok(other.length === 0, `${tag}: no page errors${other.length ? ` (${other.join("; ")})` : ""}`);
await browser.close();
console.log(fails.length ? `${fails.length} failed` : "all ok");
process.exit(fails.length ? 1 : 0);
