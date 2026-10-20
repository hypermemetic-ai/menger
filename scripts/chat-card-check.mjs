#!/usr/bin/env node
// CHAT-56: ticket cards that act, in a browser. Against a throwaway server
// holding chat CHAT whose messages name, each on a line of its own, KILN-2
// and KILN-3 (Pending), KILN-4 (Pending, a decision `cooling` with options
// `slow` (recommended) and `natural`), ZZZ-9 (no such ticket) and KILN-5 in
// a sentence only; scripts/throwaway-login.mjs makes the session file. RESET
// is a shell command that puts KILN-2..4 back to Pending and clears the
// answer. At 390 (iPhone 13, Chromium) and 1440, light and dark:
//   - a card per lone code readable (KILN-2, -3, -4), none for ZZZ-9 or
//     KILN-5; at most 560 px wide at 1440, the turn's width on a phone, its
//     actions one row (⋯ for the rest) and its questions folded there;
//   - go all 2 under the message naming two Pending tickets;
//   - go: KILN-2 → Ready, live on the card; go all then gone;
//   - drop: KILN-3 → Canceled, undo → Pending again;
//   - the decision answered on KILN-4's card: the card says the answer,
//     and the app's inputs page shows it;
//   - go all: both Ready; ask again: "@builder about KILN-2: " composed;
//   - no page errors.
// Shots to SHOTS when set.
//
//   node scripts/chat-card-check.mjs http://127.0.0.1:PORT SESSION_FILE CHAT
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
import { execSync } from "node:child_process";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const shots = process.env.SHOTS;
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const card = (id) => `#chatfull .tkcard[data-ticket="${id}"]`;
const status = (page, id) => page.$eval(card(id), (c) => c.dataset.status).catch(() => null);
const until = async (f, ms = 8000) => { const end = Date.now() + ms; while (Date.now() < end) { if (await f()) return true; await new Promise((r) => setTimeout(r, 150)); } return false; };

async function run(device, scheme) {
  if (process.env.RESET) execSync(process.env.RESET, { stdio: "ignore" });
  const browser = await pw.chromium.launch();
  const ctx = await browser.newContext({ ...device, colorScheme: scheme });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  const w = device.viewport.width;
  const phone = w < 500;
  const tag = `${w}-${scheme}`;
  await page.goto(`${origin}/#/chats/${chat}`);
  await page.waitForSelector(card("KILN-4"), { timeout: 30000 });
  await page.waitForTimeout(1200);
  const ids = await page.$$eval("#chatfull .tkcard", (cs) => cs.map((c) => c.dataset.ticket));
  ok(JSON.stringify(ids) === JSON.stringify(["KILN-2", "KILN-3", "KILN-4"]), `${tag}: a card per lone readable code (${ids.join(", ")})`);
  const box = await page.$eval(card("KILN-2"), (c) => { const r = c.getBoundingClientRect(); const t = c.closest(".turn").getBoundingClientRect(); return { w: r.width, tw: t.width, l: r.left, r: r.right }; });
  if (phone) ok(box.l >= 0 && box.r <= w && box.w >= box.tw * 0.7, `${tag}: the turn's width on a phone (${Math.round(box.w)} of ${Math.round(box.tw)})`);
  else ok(box.w <= 561, `${tag}: at most 560 wide (${Math.round(box.w)})`);
  const actH = await page.$eval(`${card("KILN-2")} .tka`, (a) => a.getBoundingClientRect().height);
  ok(actH < 48, `${tag}: the actions one row (${Math.round(actH)}px)`);
  if (phone) {
    const vis = await page.$$eval(`${card("KILN-2")} .tka > *`, (xs) => xs.filter((x) => x.offsetParent).map((x) => x.textContent));
    ok(vis.join("|") === "go|drop|⋯", `${tag}: go, drop and ⋯ on a phone (${vis.join(" ")})`);
    await page.click(`${card("KILN-2")} .tkdots`);
    const all = await page.$$eval(`${card("KILN-2")} .tka > *`, (xs) => xs.filter((x) => x.offsetParent).length);
    ok(all === 5, `${tag}: ⋯ shows the rest (${all})`);
    ok(!(await page.$eval(`${card("KILN-4")} .tkqs`, (d) => d.open)), `${tag}: questions folded on a phone`);
    ok((await page.$eval(`${card("KILN-4")} .tkqs > summary`, (s) => s.textContent)) === "1 question", `${tag}: "1 question"`);
    await page.click(`${card("KILN-4")} .tkqs > summary`);
  } else {
    ok(await page.$eval(`${card("KILN-4")} .tkqs`, (d) => d.open), `${tag}: questions open at ${w}`);
  }
  const rec = await page.$eval(`${card("KILN-4")} .tkopt .rec`, (r) => r.closest(".tkopt").textContent);
  ok(rec.startsWith("Slow"), `${tag}: the recommended option marked (${rec})`);
  const allBar = await page.$eval("#chatfull .tkall .qa", (b) => b.textContent).catch(() => null);
  ok(allBar === "go all 2", `${tag}: go all under the message naming two Pending (${allBar})`);
  if (shots) await page.screenshot({ path: `${shots}/${tag}-cards.png` });

  if (tag === "390-light") {
    await page.click(`${card("KILN-2")} .qa.go`);
    ok(await until(async () => (await status(page, "KILN-2")) === "Ready"), `${tag}: go → KILN-2 Ready, live`);
    ok(await until(async () => !(await page.$("#chatfull .tkall"))), `${tag}: go all gone with one Pending left`);
    await page.click(`${card("KILN-3")} .qa.warn`);
    ok(await until(async () => (await status(page, "KILN-3")) === "Canceled"), `${tag}: drop → KILN-3 Canceled`);
    ok(!!(await page.$(`${card("KILN-3")} .tkundo`)), `${tag}: undo offered`);
    if (shots) await page.screenshot({ path: `${shots}/${tag}-dropped.png` });
    await page.click(`${card("KILN-3")} .tkundo .qa`);
    ok(await until(async () => (await status(page, "KILN-3")) === "Pending"), `${tag}: undo → KILN-3 Pending`);
  }
  if (tag === "1440-light") {
    await page.click(`${card("KILN-4")} .tkopt:has-text("Slow")`);
    ok(await until(async () => ((await page.$eval(`${card("KILN-4")} .tkans`, (a) => a.textContent).catch(() => "")) === "answered: Slow, 50°/h")), `${tag}: the decision answered on the card, and shown`);
    ok(await page.$eval(`${card("KILN-4")} .tkopt.on`, (o) => o.textContent.startsWith("Slow")).catch(() => false), `${tag}: the chosen option marked`);
    if (shots) await page.screenshot({ path: `${shots}/${tag}-answered.png` });
    // The house's <m-decision> is a component ticket a throwaway server
    // lacks, so the app's own inputs page is where the record is read.
    await page.goto(`${origin}/#/inputs`);
    const shown = await until(async () => /KILN-4[\s\S]*(Slow, 50°\/h|slow)/.test(await page.innerText("body")));
    ok(shown, `${tag}: the answer shows in the app's inputs`);
    if (shots) await page.screenshot({ path: `${shots}/${tag}-inputs.png` });
  }
  if (tag === "1440-dark") {
    await page.click("#chatfull .tkall .qa");
    ok(await until(async () => (await status(page, "KILN-2")) === "Ready" && (await status(page, "KILN-3")) === "Ready"), `${tag}: go all → KILN-2 and KILN-3 Ready`);
    await page.click(`${card("KILN-2")} [data-act=ask]`);
    const v = await page.$eval(".ccompose .cbox", (b) => b.value ?? b.textContent);
    ok(v === "@builder about KILN-2: ", `${tag}: ask again composes (${JSON.stringify(v)})`);
  }
  ok(errs.length === 0, `${tag}: no page errors${errs.length ? ` (${errs[0]})` : ""}`);
  await browser.close();
}

const phone = pw.devices["iPhone 13"];
const desk = { viewport: { width: 1440, height: 900 } };
for (const [d, s] of [[phone, "light"], [phone, "dark"], [desk, "light"], [desk, "dark"]]) {
  try { await run(d, s); } catch (e) { ok(false, `${d.viewport.width}-${s}: ${e.message.split("\n")[0]}`); }
}
console.log(fails.length ? `${fails.length} failed` : "all ok");
process.exit(fails.length ? 1 : 0);
