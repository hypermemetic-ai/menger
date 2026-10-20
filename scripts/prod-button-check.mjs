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
//   testfailed  (DEPLOY-42) test's last deploy failed: the button is off, still a button, saying so
//   norelease   (DEPLOY-42) test's record names a commit and no release, app-prod runs it: off,
//          "→ prod · nothing newer", the reason right under it
//   retired     (DEPLOY-43) an old `prod` nothing promotes into, deployed before app-prod: no `to`
//          and ?to=app-prod open on app-prod; a saved ?to=prod is moved to it with a note; the picker
//          lists prod last, marked retired; picked, → prod is still drawn, off, saying why
// Every mode also checks the button reads as a button (DEPLOY-42): on screen, as tall as → test
// (when drawn), a full-strength label, and its reason (when off) in the text colour, not faint.
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
page.on("pageerror", (e) => { errs.push(String(e)); if (process.env.CHECK_DEBUG) console.log(e.stack); });
if (process.env.CHECK_DEBUG) page.on("console", (m) => console.log("console:", m.text().slice(0, 300)));
const shot = async (name) => out && page.screenshot({ path: `${out}/prod-${mode}-${w}-${theme}-${name}.png` });
const R = "#deploy-page .toprod .treq";
const steps = () => page.$$eval(`${R} .step`, (ls) => ls.map((l) => `${l.dataset.step}:${l.querySelector(".sstate").textContent}`));
const state = () => page.$eval(`${R} .tstate`, (e) => e.textContent).catch(() => null);
const until = async (f, ms = 40000) => {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) { if (await f()) return true; await page.waitForTimeout(120); }
  return false;
};

if (mode === "retired") {
  const btn = async (q) => {
    await page.goto(`${origin}/#/deploy/MENGER${q}`);
    await page.reload();
    await page.waitForSelector("#deploy-page #to-prod", { timeout: Number(process.env.CHECK_TIMEOUT || 30000) });
    await page.waitForFunction(() => !document.getElementById("boot"), null, { timeout: Number(process.env.CHECK_TIMEOUT || 30000) });
    await page.waitForTimeout(600);
  };
  const seen = () => page.evaluate(() => {
    const b = document.querySelector("#to-prod"), r = b.getBoundingClientRect(), t = document.querySelector("#deploy-page .totest:not(.toprod) .tobtn");
    const sel = document.querySelector("#deploy-page select[aria-label=destination]");
    return {
      dest: b.dataset.dest, disabled: b.disabled, onScreen: r.width > 40 && r.height >= 30 && r.left >= 0 && r.right <= innerWidth,
      sameHeight: t ? Math.abs(t.getBoundingClientRect().height - r.height) <= 2 : true,
      note: document.querySelector("#deploy-page .movednote")?.textContent ?? null,
      hash: location.hash, options: sel ? [...sel.options].map((o) => o.textContent) : [], chosen: sel?.value,
      why: document.querySelector("#deploy-page .toprod .toff")?.textContent ?? null,
    };
  });
  for (const q of ["", "?to=app-prod", "?to=prod"]) {
    await btn(q);
    const v = await seen();
    console.log(`errors after ${q || "(no to)"}: ${errs.length}`);
    ok(v.dest === "app-prod" && v.onScreen && v.sameHeight, `${tag} ${q || "(no to)"}: opens on app-prod with → prod drawn (${JSON.stringify(v)})`);
    ok(v.options[0] === "app-prod" && v.options.at(-1) === "prod (retired)", `${tag} ${q || "(no to)"}: the picker lists prod last, retired (${v.options.join()})`);
    ok(q === "?to=prod" ? v.note === "prod moved to app-prod (RUN-13)" && /to=app-prod/.test(v.hash) : v.note === null, `${tag} ${q || "(no to)"}: the note is there only for a stale ?to=prod (${v.note})`);
    if (q === "?to=prod") await shot("moved");
  }
  await page.selectOption("#deploy-page select[aria-label=destination]", "prod");
  await page.waitForTimeout(600);
  const v = await seen();
  console.log(`errors after picking prod: ${errs.length}`);
  ok(v.dest === "prod" && v.disabled && v.onScreen && v.sameHeight && /pick app-prod/.test(v.why ?? ""), `${tag}: prod picked: → prod still drawn, off, saying why (${JSON.stringify(v)})`);
  await shot("prod-picked");
  ok(errs.length === 0, `${tag}: no page errors${errs.length ? ` (${errs.join("; ")})` : ""}`);
  await browser.close();
  console.log(fails.length ? `${fails.length} failed` : "all ok");
  process.exit(fails.length ? 1 : 0);
}

await page.goto(`${origin}/#/deploy/MENGER?to=app-prod`);
await page.waitForSelector("#deploy-page #to-prod", { timeout: Number(process.env.CHECK_TIMEOUT || 30000) }).catch(async (e) => {
  console.log("no #to-prod; the page says:", (await page.innerText("body").catch(() => "")).slice(0, 600));
  throw e;
});
await page.waitForFunction(() => !document.getElementById("boot"), null, { timeout: Number(process.env.CHECK_TIMEOUT || 30000) }); // APP-92's sheet gone
ok(await page.$eval("#to-prod", (b) => b.dataset.dest) === "app-prod", `${tag}: → prod promotes to the track's end, app-prod`);

// DEPLOY-42: a button, not a ghost — visible, test's size, full strength, its reason in the ink.
const look = await page.evaluate(() => {
  const b = document.querySelector("#to-prod"), r = b.getBoundingClientRect(), cs = getComputedStyle(b);
  const t = document.querySelector("#deploy-page .totest:not(.toprod) .tobtn"), off = document.querySelector("#deploy-page .toprod .toff");
  const ink = getComputedStyle(document.querySelector("#deploy-page")).color;
  return {
    onScreen: r.width > 40 && r.height >= 30 && r.left >= 0 && r.right <= innerWidth && getComputedStyle(b).visibility === "visible",
    opacity: Number(cs.opacity), sameHeight: t ? Math.abs(t.getBoundingClientRect().height - r.height) <= 2 : true,
    reasonInk: off ? getComputedStyle(off).color === ink && Number(getComputedStyle(off).opacity) === 1 : null,
    reasonBelow: off ? off.getBoundingClientRect().top >= r.bottom - 1 && off.getBoundingClientRect().top - r.bottom < 24 : null,
    disabled: b.disabled,
  };
});
ok(look.onScreen && look.opacity === 1, `${tag}: → prod is on screen, a full-strength button (${JSON.stringify(look)})`);
ok(look.sameHeight, `${tag}: as tall as → test`);
ok(look.reasonInk !== false && look.reasonBelow !== false, `${tag}: its reason, when there is one, sits right under it in the text colour`);
ok(look.disabled === (mode === "none" || mode === "testfailed" || mode === "norelease"), `${tag}: off exactly when it should be`);

if (mode === "testfailed") {
  ok(/test's last deploy failed/.test(await page.textContent("#deploy-page .toprod .toff")), `${tag}: the reason says test's deploy failed`);
  ok((await page.textContent("#to-prod")) === "→ prod", `${tag}: the label is → prod`);
  await shot("off");
} else if (mode === "norelease") {
  ok((await page.textContent("#to-prod")) === "→ prod · nothing newer", `${tag}: the label says nothing newer`);
  ok(/app-prod runs \S+ already: test has nothing newer/.test(await page.textContent("#deploy-page .toprod .toff")), `${tag}: the reason names the commit app-prod runs`);
  await shot("off");
} else if (mode === "none") {
  ok((await page.textContent("#to-prod")) === "→ prod · nothing newer", `${tag}: the label says nothing newer`);
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
