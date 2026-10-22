#!/usr/bin/env node
// FILTER-49: the filter menu is a popover (a sheet on a phone) — Filter
// opens it over the board without moving it, its categories open one at a
// time with counts and is / is not, the search narrows every value, Escape
// and a press outside close it, `f` opens it again, edit as text is at its
// foot. Against a throwaway (scripts/filter-menu-check.sh); screenshots of
// open / a category open / search typed when SHOTS is given.
//
//   node scripts/filter-menu-check.mjs ORIGIN SESSION_FILE WIDTH THEME [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, widthArg, scheme, shots] = process.argv.slice(2);
const width = Number(widthArg);
const phone = width <= 820;
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width, height: phone ? 844 : 900 }, colorScheme: scheme, hasTouch: phone });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const tag = `${width}-${scheme}`;
const shot = async (name) => { if (shots) await page.screenshot({ path: `${shots}/${name}-${tag}.png` }); };
await page.goto(`${origin}/app/#/`);
const btn = '.fline [data-fbtn="board"]';
try {
  await page.waitForSelector(btn, { timeout: 30000 });
  await page.waitForFunction(() => /live/.test(document.querySelector("#conn")?.textContent ?? "live"), null, { timeout: 15000 });
  await page.waitForSelector("#board tbody tr, .board tbody tr, table tbody tr", { timeout: 15000 });
} catch {
  await shot("none");
  console.log(`FAIL ${tag}: the board's Filter did not draw (${errs.join(" | ")})`);
  process.exit(1);
}
await page.evaluate(() => document.fonts.ready);
await page.waitForTimeout(600);

const menu = ".fmenu";
const boardTop = () => page.evaluate(() => document.querySelector("table")?.getBoundingClientRect().top ?? -1);
const isOpen = () => page.locator(menu).count().then((n) => n > 0);

// Opened from Filter: over the board, nothing moved.
const before = await boardTop();
await page.click(btn);
await page.waitForSelector(menu);
await page.waitForTimeout(400);
ok(await isOpen(), `${tag}: Filter opens the menu`);
ok(Math.abs((await boardTop()) - before) < 1, `${tag}: the board does not move under it (${before} → ${await boardTop()})`);
if (phone) {
  ok(await page.locator(`#lens-top ${menu}`).count() === 1, `${tag}: on a phone the menu is the filter sheet's`);
  ok(await page.evaluate(() => document.querySelector("#app")?.dataset.sheet === "filter"), `${tag}: the sheet is up`);
} else {
  const pos = await page.$eval(menu, (m) => getComputedStyle(m).position);
  ok(pos === "absolute", `${tag}: a popover (position ${pos})`);
  const [mb, bb] = await Promise.all([page.$eval(menu, (m) => m.getBoundingClientRect().toJSON()), page.$eval(btn, (b) => b.getBoundingClientRect().toJSON())]);
  ok(mb.top >= bb.bottom && mb.top - bb.bottom < 12, `${tag}: it hangs under its button (${Math.round(mb.top)} vs ${Math.round(bb.bottom)})`);
}
const rows = await page.locator(`${menu} .fmain .frow.cat`).allInnerTexts();
ok(rows.length >= 8 && rows.length <= 14, `${tag}: a short list of categories (${rows.length})`);
ok(/saved/i.test(rows[0] ?? ""), `${tag}: the saved filters first (${(rows[0] ?? "").trim()})`);
ok(await page.locator(`${menu} .fsub`).count() === 0, `${tag}: opened on its list, no category open`);
ok(await page.locator(`${menu} .fp-text`).count() === 1, `${tag}: edit as text at its foot`);
const sw = await page.evaluate(() => ({ sw: document.scrollingElement.scrollWidth, iw: innerWidth }));
ok(sw.sw <= sw.iw, `${tag}: no horizontal scroll (${sw.sw} <= ${sw.iw})`);
await shot("01-open");

// A category, opened: its values counted, is / is not in its head.
await page.click(`${menu} .frow.cat[data-sec="status"]`);
await page.waitForSelector(`${menu} .fsub [data-cat="status"].fv`);
await page.waitForTimeout(500);
ok(await page.locator(`${menu} .fsub .fop`).count() === 1, `${tag}: is / is not in the category's head`);
const counted = await page.locator(`${menu} .fsub .fv .fcount`).allInnerTexts();
ok(counted.some((t) => /\d/.test(t)), `${tag}: the values are counted (${counted.slice(0, 4).join(",")})`);
ok(await page.locator(`${menu} .fsub`).count() === 1, `${tag}: one category open`);
if (phone) ok(!(await page.locator(`${menu} .fmain`).isVisible()), `${tag}: on a phone the values take the list's place`);
await shot("02-category");
// Another category replaces it.
if (phone) await page.click(`${menu} .fsub .frow.back`);
await page.click(`${menu} .frow.cat[data-sec="kind"]`);
await page.waitForSelector(`${menu} .fsub [data-cat="kind"].fv`);
ok(await page.locator(`${menu} .fsub [data-cat="status"]`).count() === 0, `${tag}: one at a time`);
// A tick: the line's chip, the board still where it was.
await page.click(`${menu} .fsub .fv[data-value="build"]`);
await page.waitForTimeout(500);
ok(await page.locator(".fline .fchip").count() >= 1, `${tag}: a tick is a chip on the line`);
ok(await page.locator(`${menu} .finto`).count() === 1, `${tag}: which group a tick goes into, once there is one`);
if (phone) await page.click(`${menu} .fsub .frow.back`);

// The search narrows every value.
await page.fill(`${menu} input.fsearch`, "kil");
await page.waitForTimeout(400);
const found = await page.locator(`${menu} .fv`).allInnerTexts();
ok(found.length >= 1 && found.every((t) => /kil/i.test(t)), `${tag}: the search narrows every value (${found.map((t) => t.split("\n")[0]).join(",")})`);
await shot("03-search");
await page.fill(`${menu} input.fsearch`, "");

// Escape closes; f opens it again; a press outside closes it.
await page.keyboard.press("Escape");
await page.waitForTimeout(300);
ok(!(await isOpen()), `${tag}: Escape closes it`);
await page.evaluate(() => document.activeElement?.blur());
await page.keyboard.press("f");
await page.waitForTimeout(400);
ok(await isOpen(), `${tag}: f opens it`);
if (!phone) {
  await page.mouse.click(width - 40, 860);
  await page.waitForTimeout(300);
  ok(!(await isOpen()), `${tag}: a press outside closes it`);
}
ok(errs.length === 0, `${tag}: no page errors (${errs.join(" | ")})`);
await browser.close();
if (fails.length) { console.log(`${fails.length} failed`); process.exit(1); }
