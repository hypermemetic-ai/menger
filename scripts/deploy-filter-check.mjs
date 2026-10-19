#!/usr/bin/env node
// DEPLOY-39: #/deploy's stage pills filter — checked headless against a
// throwaway server with work in flight (scripts/throwaway-login.mjs makes the
// session file):
//   node scripts/deploy-filter-check.mjs http://127.0.0.1:PORT SESSION_FILE WIDTH THEME [OUTDIR]
// Checks, per pill: only its tickets are drawn (list rows at a phone, graph
// nodes at a desk), the areas shown hold matches only and their counts are of
// them (an area with none is no section at all), the address says ?stage=…, a reload keeps
// it, a second tap clears it and everything is back, the page's scroll is
// kept, and no page errors. Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, width, theme, out] = process.argv.slice(2);
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
const shot = async (name) => out && page.screenshot({ path: `${out}/${w}-${theme}-${name}.png` });
const sel = phone ? "#deploy-page .list .row" : "#deploy-page .canvas .fnode";
const states = ["coding", "review", "waiting", "ready", "deploying", "failed"];

const open = async (hash) => {
  await page.goto(`${origin}/app/${hash}`);
  await page.waitForSelector("#deploy-page", { timeout: 30000 });
  if (!phone) await page.click("#seg-graph").catch(() => {});
  await page.waitForSelector(sel, { timeout: 30000 });
  await page.waitForTimeout(800);
};
const pill = (s) => `#deploy-page .legend button[data-state="${s}"]`;
const read = () => page.evaluate(({ sel, phone }) => {
  const shown = [...document.querySelectorAll(sel)].map((n) => n.dataset.ticketId);
  const areas = phone
    ? [...document.querySelectorAll("#deploy-page .list .cl")].map((c) => ({ code: c.dataset.area, n: +c.querySelector(".clh .n").textContent, rows: c.querySelectorAll(".row").length }))
    : [...document.querySelectorAll("#deploy-page .canvas .hull")].map((h) => ({ code: h.dataset.area, n: +h.querySelector(".hsub").textContent.match(/^(\d+) ticket/)[1] }));
  const dim = document.querySelectorAll("#deploy-page .dim").length;
  return { shown, areas, dim, hash: location.hash };
}, { sel, phone });
const count = async (s) => +(await page.textContent(`${pill(s)} .n`));

await open("#/deploy");
const all = await read();
ok(all.shown.length > 0 && all.areas.length >= 2, `${tag}: unfiltered, ${all.shown.length} tickets in ${all.areas.length} areas`);
await shot("all");
const total = {};
for (const s of states) total[s] = await count(s);

for (const s of states) {
  if (!total[s]) continue;
  await page.click(pill(s));
  await page.waitForTimeout(400);
  const r = await read();
  ok(r.shown.length === total[s], `${tag}: ${s}: ${r.shown.length} drawn = the pill's ${total[s]}`);
  ok(r.dim === 0, `${tag}: ${s}: nothing is dimmed`);
  ok(r.areas.every((a) => a.n > 0) && r.areas.reduce((t, a) => t + a.n, 0) === total[s], `${tag}: ${s}: area counts add up to the matches (${r.areas.map((a) => `${a.code} ${a.n}`).join(", ")})`);
  if (phone) ok(r.areas.every((a) => a.rows === a.n), `${tag}: ${s}: each area's rows are its count`);
  ok(r.hash.includes(`stage=${s}`), `${tag}: ${s}: the address says it (${r.hash})`);
  await shot(s);
  await page.reload();
  await page.waitForSelector(sel, { timeout: 30000 });
  await page.waitForTimeout(800);
  const again = await read();
  ok(again.shown.length === total[s] && (await page.getAttribute(pill(s), "aria-pressed")) === "true", `${tag}: ${s}: a reload keeps the filter`);
  await page.click(pill(s));
  await page.waitForTimeout(400);
  const back = await read();
  ok(back.shown.length === all.shown.length && !back.hash.includes("stage="), `${tag}: ${s}: tapping again brings all ${back.shown.length} back, the address clean`);
}

if (phone) {
  await open("#/deploy");
  await page.click(pill("waiting"));
  await page.waitForTimeout(300);
  await page.evaluate(() => window.scrollTo(0, 120));
  const y0 = await page.evaluate(() => window.scrollY);
  const marks = await page.evaluate(() => { const el = document.querySelector("#deploy-page .totest"); el.dataset.kept = "1"; return 1; });
  await page.click(pill("ready"));
  await page.waitForTimeout(300);
  ok(await page.evaluate(() => document.querySelector("#deploy-page .totest")?.dataset.kept === "1"), `${tag}: changing the filter leaves the rest of the page's DOM alone`);
  ok((await page.evaluate(() => window.scrollY)) <= y0 && marks === 1, `${tag}: scroll kept as far as the shorter page allows`);
}
ok(errs.length === 0, `${tag}: no page errors${errs.length ? ` (${errs[0]})` : ""}`);
await browser.close();
if (fails.length) { console.log(`\n${fails.length} FAILED`); process.exit(1); }
console.log("\nall ok");
