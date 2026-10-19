#!/usr/bin/env node
// DEPLOY-37: #/deploy's → test, checked headless against a throwaway server
// whose project has a `test` environment, its webhook set to a loopback
// deploy/runner.mjs --listen --fake (deploy/README "The deploy runner"):
//   node scripts/deploy-button-check.mjs http://127.0.0.1:PORT SESSION_FILE MODE WIDTH THEME [OUTDIR]
// MODE: `ok` (the request runs to its end: every step drawn live, the
// button off while it is open and on again after), `cancel` (canceled
// during its build: the build canceled, the rest skipped or canceled, the
// request canceled) or `fail` (the runner started with
// MENGER_RUNNER_FAKE_FAIL=build: the build failed, its last lines shown).
// Each mode files one request on the throwaway. Exit 1 on a failure.
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
const shot = async (name) => out && page.screenshot({ path: `${out}/${mode}-${w}-${theme}-${name}.png` });
const steps = () => page.$$eval("#deploy-page .treq .step", (ls) => ls.map((l) => `${l.dataset.step}:${l.querySelector(".sstate").textContent}`));
const until = async (what, f, ms = 30000) => {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) { if (await f()) return true; await page.waitForTimeout(150); }
  return false;
};

await page.goto(`${origin}/#/deploy`);
await page.waitForSelector("#deploy-page #to-test", { timeout: 30000 });
await page.waitForFunction(() => !document.querySelector("#to-test").disabled, null, { timeout: 30000 });
ok(true, `${tag}: an admin sees → test, and it is on`);
const before = await page.$eval("#deploy-page", () => document.querySelector("#deploy-page .treq")?.dataset.request ?? null);
await page.click("#to-test");
const filed = await until("a request", async () => (await page.$eval("#deploy-page", () => document.querySelector("#deploy-page .treq")?.dataset.request ?? null)) !== before);
ok(filed, `${tag}: pressing it files a request, drawn under the button`);
ok(await page.$eval("#to-test", (b) => b.disabled && /is (waiting|CI started)/.test(b.title)), `${tag}: the button is off while the request is open, saying why`);
ok(await until("build started", async () => (await steps()).includes("build:started")), `${tag}: the executor's build step is drawn live, under way`);
await shot("build");

if (mode === "ok") {
  ok(await until("deploy started", async () => (await steps()).join() === "build:succeeded,deploy:started"), `${tag}: the build succeeds, the deploy starts (${(await steps()).join()})`);
  await shot("deploy");
  ok(await until("done", async () => (await page.textContent("#deploy-page .treq .tstate")) === "CI succeeded"), `${tag}: the request ends succeeded`);
  ok((await steps()).join() === "build:succeeded,deploy:succeeded", `${tag}: both steps succeeded (${(await steps()).join()})`);
  ok(await page.$$eval("#deploy-page .treq .slog", (as) => as.length === 2 && as.every((a) => /\/runs\/\d+\/(build|deploy)\.log$/.test(a.href))), `${tag}: each step links its log`);
  ok(await until("on again", () => page.$eval("#to-test", (b) => !b.disabled)), `${tag}: the button is on again once it ended`);
} else if (mode === "cancel") {
  await page.click("#deploy-page .treq .tcancelbtn");
  ok(await until("asked", async () => (await page.$("#deploy-page .treq .tcancel")) !== null || (await page.textContent("#deploy-page .treq .tstate")) === "canceled"), `${tag}: cancel asks the executor to stop`);
  ok(await until("canceled", async () => (await steps()).join() === "build:canceled,deploy:canceled" || (await steps()).join() === "build:canceled,deploy:skipped"), `${tag}: the build is canceled, the deploy never runs (${(await steps()).join()})`);
  ok(await until("words", async () => (await page.textContent("#deploy-page .treq .tstate")) === "canceled" && !(await page.$("#deploy-page .treq .tcancel"))), `${tag}: the request says canceled, no longer stopping`);
  await shot("canceled");
  ok(await until("on again", () => page.$eval("#to-test", (b) => !b.disabled)), `${tag}: the button is on again`);
} else if (mode === "fail") {
  ok(await until("failed", async () => (await page.textContent("#deploy-page .treq .tstate")) === "CI failed"), `${tag}: the request ends failed`);
  ok((await steps()).join() === "build:failed,deploy:skipped", `${tag}: the build failed, the deploy skipped (${(await steps()).join()})`);
  ok(/FAILED: the fake's failure/.test(await page.textContent("#deploy-page .treq .step.failed .sdetail")), `${tag}: the failed step's last lines are shown`);
  await shot("failed");
}

const over = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
ok(over <= 0, `${tag}: nothing wider than the page (${over} px over)`);
const box = await page.$eval("#deploy-page .totest", (e) => { const r = e.getBoundingClientRect(); return r.right <= innerWidth && r.left >= 0; });
ok(box, `${tag}: the button and its request sit inside the viewport`);
ok(errs.length === 0, `${tag}: no page errors${errs.length ? ` (${errs.join("; ")})` : ""}`);
await browser.close();
console.log(fails.length ? `${fails.length} failed` : "all ok");
process.exit(fails.length ? 1 : 0);
