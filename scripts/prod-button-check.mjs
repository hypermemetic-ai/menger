#!/usr/bin/env node
// DEPLOY-40: #/deploy's → prod, checked headless against a throwaway server
// whose project has test → app-test → app-prod, app-prod's webhook set to a
// loopback deploy/runner.mjs --listen --fake --owner idp:<the owner>
// (scripts/prod-button-check.sh sets it all up and runs every mode):
//   node scripts/prod-button-check.mjs http://127.0.0.1:PORT SESSION_FILE MODE WIDTH THEME [OUTDIR]
// MODE:
//   ok     the owner: the label names test's release; the confirm sheet lists what goes in,
//          grouped by area, with the release and its commit; confirm files the request and
//          image → app-test → app-prod draw live, in order, to succeeded; then the button is
//          off, saying app-prod runs it already
//   fail   the owner, the runner with MENGER_RUNNER_FAKE_FAIL=app-test: image succeeded,
//          app-test failed with its last lines, app-prod skipped, the request failed
//   agent  an agent's key (an admin): the request is filed, the runner refuses it in its
//          first step ("is a key"), the rest skipped, nothing runs
//   none   the button is off with its reason drawn beside it: nothing newer than app-prod
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, mode, width, theme, out] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const w = Number(width);
const tag = `${mode} ${w} ${theme}`;

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width: w, height: w < 820 ? 844 : 900 }, deviceScaleFactor: 2, hasTouch: w < 820, isMobile: w < 820, colorScheme: theme });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const shot = async (name) => out && page.screenshot({ path: `${out}/prod-${mode}-${w}-${theme}-${name}.png` });
const R = "#deploy-page .toprod .treq";
const steps = () => page.$$eval(`${R} .step`, (ls) => ls.map((l) => `${l.dataset.step}:${l.querySelector(".sstate").textContent}`));
const state = () => page.$eval(`${R} .tstate`, (e) => e.textContent).catch(() => null);
const until = async (f, ms = 40000) => {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) { if (await f()) return true; await page.waitForTimeout(120); }
  return false;
};

await page.goto(`${origin}/#/deploy/MENGER?to=app-prod`);
await page.waitForSelector("#deploy-page #to-prod", { timeout: 30000 });
await page.waitForFunction(() => !document.getElementById("boot"), null, { timeout: 30000 }); // APP-92's sheet gone
ok(await page.$eval("#to-prod", (b) => b.dataset.dest) === "app-prod", `${tag}: → prod promotes to the track's end, app-prod`);

if (mode === "none") {
  ok(await until(() => page.$eval("#to-prod", (b) => b.disabled && /runs \S+ already: test has nothing newer/.test(b.title))), `${tag}: the button is off, its title says why`);
  ok(/runs \S+ already/.test(await page.textContent("#deploy-page .toprod .toff")), `${tag}: and the reason is drawn beside it`);
  await shot("off");
} else {
  ok(await until(() => page.$eval("#to-prod", (b) => !b.disabled)), `${tag}: the button is on`);
  const label = await page.textContent("#to-prod");
  ok(/^→ prod · \d{8}T\d{6}Z-[0-9a-f]{7}$/.test(label), `${tag}: its label names the release test passed (${label})`);
  const rel = label.split(" · ")[1];
  const before = await page.$eval("#deploy-page", () => document.querySelector("#deploy-page .toprod .treq")?.dataset.request ?? null);
  await page.click("#to-prod");
  await page.waitForSelector("#prod-sheet", { timeout: 10000 });
  ok(await until(async () => (await page.$$("#prod-sheet .pgroup li")).length > 0), `${tag}: the confirm sheet lists what goes in`);
  const sheet = await page.$eval("#prod-sheet", (s) => ({
    rel: s.querySelector(".prelname")?.textContent, commit: s.querySelector(".pcommit")?.textContent,
    groups: [...s.querySelectorAll(".pgroup")].map((g) => `${g.dataset.code}:${[...g.querySelectorAll("li")].map((l) => l.dataset.ticketId).join("+")}`),
    inside: (() => { const r = s.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth + 0.5 && r.bottom <= innerHeight + 0.5; })(),
  }));
  ok(sheet.rel === rel && sheet.commit === rel.split("-").pop(), `${tag}: with the release and its commit (${sheet.rel} ${sheet.commit})`);
  ok(sheet.groups.join(" ") === "KILN:KILN-2+KILN-3 GLAZE:GLAZE-2", `${tag}: grouped by area as the page groups them (${sheet.groups.join(" ")})`);
  ok(sheet.inside, `${tag}: the sheet sits inside the viewport`);
  await shot("sheet");
  await page.click("#prod-confirm");
  ok(await until(async () => (await page.$("#prod-sheet")) === null), `${tag}: confirm closes the sheet`);
  const filed = await until(async () => (await page.$eval("#deploy-page", () => document.querySelector("#deploy-page .toprod .treq")?.dataset.request ?? null)) !== before);
  ok(filed, `${tag}: confirm files the request, drawn under the button`);
  ok(await page.$eval(`${R} .twhat`, (e) => e.textContent).then((t) => t.includes(rel) && t.includes("app-prod")), `${tag}: the request names the release and app-prod`);

  if (mode === "ok") {
    ok(await until(() => page.$eval("#to-prod", (b) => b.disabled && /is (waiting|CI started)/.test(b.title))), `${tag}: the button is off while the request is open`);
    const seen = new Set();
    const watch = async () => { const s = (await steps()).join(); seen.add(s); return (await state()) === "CI succeeded"; };
    ok(await until(watch), `${tag}: the request ends succeeded`);
    const order = ["image:started", "image:succeeded,app-test:started", "image:succeeded,app-test:succeeded,app-prod:started"];
    ok(order.every((s) => seen.has(s)), `${tag}: the steps draw live, one after another (${order.filter((s) => !seen.has(s)).join(" | ") || "all seen"})`);
    ok((await steps()).join() === "image:succeeded,app-test:succeeded,app-prod:succeeded", `${tag}: image, app-test, app-prod succeeded (${(await steps()).join()})`);
    await shot("done");
    ok(await until(() => page.$eval("#to-prod", (b) => b.disabled && /app-prod runs \S+ already/.test(b.title))), `${tag}: then the button is off: app-prod runs it already`);
  } else if (mode === "fail") {
    ok(await until(async () => (await state()) === "CI failed"), `${tag}: the request ends failed`);
    ok((await steps()).join() === "image:succeeded,app-test:failed,app-prod:skipped", `${tag}: a failing step stops the plan (${(await steps()).join()})`);
    ok(/FAILED: the fake's failure/.test(await page.textContent(`${R} .step.failed .sdetail`)), `${tag}: the failed step's last lines are shown`);
    await shot("failed");
    ok(await until(() => page.$eval("#to-prod", (b) => !b.disabled)), `${tag}: the button is on again: app-prod still runs what it ran`);
  } else if (mode === "agent") {
    ok(await until(async () => (await state()) === "CI failed"), `${tag}: the request ends failed`);
    ok((await steps()).join() === "image:failed,app-test:skipped,app-prod:skipped", `${tag}: refused in its first step, nothing runs (${(await steps()).join()})`);
    ok(/^refused: key:[0-9a-f]+ is a key/.test(await page.textContent(`${R} .step.failed .sdetail`)), `${tag}: the refusal says an agent's key may not`);
    await shot("refused");
  }
}

const over = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
ok(over <= 0, `${tag}: nothing wider than the page (${over} px over)`);
ok(await page.$eval("#deploy-page .toprod", (e) => { const r = e.getBoundingClientRect(); return r.right <= innerWidth && r.left >= 0; }), `${tag}: the button and its request sit inside the viewport`);
ok(errs.length === 0, `${tag}: no page errors${errs.length ? ` (${errs.join("; ")})` : ""}`);
await browser.close();
console.log(fails.length ? `${fails.length} failed` : "all ok");
process.exit(fails.length ? 1 : 0);
