#!/usr/bin/env node
// COMP-17: #/components has no horizontal overflow at 390 and 1440, light and
// dark; every card sits inside the viewport; a component referenced by 200+
// tickets shows a count that expands to a wrapped chip list (the first 50,
// then "show all") and collapses; the search narrows the list. Against a
// throwaway (scripts/components-check.sh).
//
//   node scripts/components-check.mjs ORIGIN SESSION_FILE WIDTH THEME [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, widthArg, scheme, shots] = process.argv.slice(2);
const width = Number(widthArg);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width, height: width === 390 ? 844 : 900 }, colorScheme: scheme, hasTouch: width === 390 });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const tag = `${width}-${scheme}`;
await page.goto(`${origin}/app/#/components`);
try {
  await page.waitForSelector("#components .comp", { timeout: 30000 });
} catch {
  if (shots) await page.screenshot({ path: `${shots}/${tag}-none.png` });
  console.log(`FAIL ${tag}: no component card drew (${errs.join(" | ")})`);
  process.exit(1);
}
await page.waitForTimeout(800);

const overflow = () => page.evaluate(() => ({ sw: document.scrollingElement.scrollWidth, iw: innerWidth }));
const cardsInside = () => page.evaluate(() => [...document.querySelectorAll("#components .comp")].filter((c) => {
  const b = c.getBoundingClientRect();
  return b.left < -0.5 || b.right > innerWidth + 0.5;
}).length);
const cardOf = (name) => `#components .comp[data-component="${name}"]`;

let o = await overflow();
ok(o.sw <= o.iw, `${tag}: no horizontal scroll (${o.sw} <= ${o.iw})`);
ok((await cardsInside()) === 0, `${tag}: every card sits inside the viewport`);
const n = await page.locator("#components .comp").count();
ok(n >= 3, `${tag}: ${n} cards`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-list.png` });

// The big one: a count, collapsed, then a wrapped list of 50, then all.
const big = cardOf("m-kiln-load");
const refs = page.locator(`${big} details.refs`);
ok((await refs.locator("summary").innerText()).includes("230 references"), `${tag}: the count reads 230 references`);
ok(!(await refs.evaluate((d) => d.open)), `${tag}: the references start collapsed`);
ok((await refs.locator(".chips .code").count()) === 50 || !(await refs.evaluate((d) => d.open)), `${tag}: nothing but the count while collapsed`);
await refs.locator("summary").click();
ok(await refs.evaluate((d) => d.open), `${tag}: the count expands`);
ok((await refs.locator(".chips .code").count()) === 50, `${tag}: the first 50 chips`);
o = await overflow();
ok(o.sw <= o.iw && (await cardsInside()) === 0, `${tag}: expanded, still no horizontal scroll (${o.sw} <= ${o.iw})`);
const rows = await refs.locator(".chips .code").evaluateAll((els) => new Set(els.map((e) => Math.round(e.getBoundingClientRect().top))).size);
ok(rows > 1, `${tag}: the chips wrap onto ${rows} lines`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-expanded.png` });
await refs.locator("button.show-all").click();
ok((await refs.locator(".chips .code").count()) === 230, `${tag}: show all draws 230`);
o = await overflow();
ok(o.sw <= o.iw && (await cardsInside()) === 0, `${tag}: all shown, still no horizontal scroll (${o.sw} <= ${o.iw})`);
await refs.locator("summary").click();
ok(!(await refs.evaluate((d) => d.open)), `${tag}: the count collapses`);

// The summary clamps to two lines; a tap opens it.
const sum = page.locator(`${big} .c-summary`);
const h0 = (await sum.boundingBox()).height;
await sum.click();
const h1 = (await sum.boundingBox()).height;
ok(width > 820 || h1 > h0, `${tag}: a tap opens the clamped summary (${Math.round(h0)} -> ${Math.round(h1)} px)`);
await sum.click();

// Search narrows.
const search = page.locator("#components input.comp-search");
await search.fill("cone");
await page.waitForTimeout(300);
ok((await page.locator("#components .comp").count()) === 1, `${tag}: "cone" narrows to one card`);
await search.fill("KILN-");
await page.waitForTimeout(300);
ok((await page.locator("#components .comp").count()) === 3, `${tag}: searching the owner finds all three`);
await search.fill("zzzzzz");
await page.waitForTimeout(300);
ok((await page.locator("#components .comp").count()) === 0 && (await page.locator("#components .nomatch").count()) === 1, `${tag}: no match says so`);
await search.fill("");
await page.waitForTimeout(300);
ok((await page.locator("#components .comp").count()) === n, `${tag}: clearing the search brings every card back`);
o = await overflow();
ok(o.sw <= o.iw, `${tag}: no horizontal scroll at the end`);
ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
await browser.close();
process.exit(fails.length ? 1 : 0);
