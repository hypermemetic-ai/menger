#!/usr/bin/env node
// TETHER-11: #/pair, checked headless against a throwaway server
// (scripts/pair-check.sh sets it up and runs every mode):
//   node scripts/pair-check.mjs http://127.0.0.1:PORT SESSION_FILE MODE WIDTH [OUTDIR]
// MODE:
//   alice    an admin of KILN: the fixture key's code is MMG4-2KLG (what tether prints);
//            a fresh machine's code is its SHA-256's base32; editor on KILN is offered;
//            Pair grants it (members.list, read by grants() in the shell, says so)
//   notmine  Not mine: dismissed, nothing granted
//   bob      a viewer: told who can grant, no Pair
//   bad      a link with no key: says so
// TETHER-12:
//   code     #/pair with no query: the code of a machine waiting (PAIR_WAITING), typed lower case
//            with no dash, finds it; the confirm screen says it matched and Pair it grants it
//   key      its key pasted as bare hex: the confirm screen, its name from the server, asked to match
//   unknown  a code no machine shows: refused in band, nothing granted
//   signin   no session: the link goes to the sign-in, its social button (the memory identity's,
//            as WorkOS's) comes back to #/pair with the query whole
// Every mode: nothing wider than the page, the buttons inside the viewport, no page errors; at 390
// the confirm screen's controls clear an iPhone's corners and home indicator (APP-66) above the bar.
import { createRequire } from "node:module";
import { createHash, randomBytes } from "node:crypto";
import { execSync } from "node:child_process";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, mode, width, out] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const w = Number(width);
const tag = `${mode} ${w}`;

// The code, as tether and the app compute it: base32 of SHA-256 of the key's bytes, 8 characters.
const code = (hex) => {
  const d = createHash("sha256").update(Buffer.from(hex, "hex")).digest();
  const A = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0n;
  for (const b of d.subarray(0, 5)) bits = (bits << 8n) | BigInt(b);
  const s = [...Array(8)].map((_, i) => A[Number((bits >> BigInt(35 - 5 * i)) & 31n)]).join("");
  return `${s.slice(0, 4)}-${s.slice(4)}`;
};
const FIXTURE = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
ok(code(FIXTURE) === "MMG4-2KLG", `${tag}: the reference code of the fixture is tether's (${code(FIXTURE)})`);
const waiting = process.env.PAIR_WAITING || "";
const machine = mode === "code" || mode === "key" ? waiting.replace(/^key:/, "") : mode === "alice" || mode === "notmine" || mode === "signin" ? randomBytes(32).toString("hex") : FIXTURE;
const principal = `key:${machine}`;
const grantsOf = () => execSync("bash -c grants", { encoding: "utf8" }).split("\n").filter((l) => l.includes(principal));

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width: w, height: w < 820 ? 844 : 900 }, deviceScaleFactor: 2, hasTouch: w < 820, isMobile: w < 820 });
if (mode !== "signin") await ctx.addCookies([{ name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" }]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
// APP-66: at 390 an iPhone's insets (the app's --sai-*), and every control
// named on the first screen, clear of the rounded corners, the home
// indicator and the phone bar.
const PHONE = { w: 390, h: 844, r: 47, top: 47, bottom: 34 };
const inCorner = (x, y) => {
  const r = PHONE.r, cx = Math.min(Math.max(x, r), PHONE.w - r), cy = Math.min(Math.max(y, r), PHONE.h - r);
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r + 0.5;
};
async function safeArea(sels, what) {
  if (w !== 390) return;
  await page.addStyleTag({ content: `:root{--sai-top:${PHONE.top}px;--sai-bottom:${PHONE.bottom}px;--sai-left:0px;--sai-right:0px}` });
  await page.waitForTimeout(300);
  const bar = await page.$eval("#pbar", (e) => e.getBoundingClientRect().top).catch(() => PHONE.h - PHONE.bottom);
  for (const s of sels) {
    const r = await page.$eval(s, (e) => e.getBoundingClientRect().toJSON());
    const corners = [[r.x, r.y], [r.right, r.y], [r.x, r.bottom], [r.right, r.bottom]];
    ok(corners.every(([x, y]) => inCorner(x, y)) && r.y >= PHONE.top - 0.5 && r.bottom <= Math.min(bar, PHONE.h - PHONE.bottom) + 0.5,
      `${tag}: ${what}'s ${s} clears the corners, the top inset, the home indicator and the bar (${r.y.toFixed(0)}–${r.bottom.toFixed(0)}, bar at ${bar.toFixed(0)})`);
  }
}
const shot = async (name) => out && page.screenshot({ path: `${out}/pair-${mode}-${w}-${name}.png` });
const hash = mode === "bad" ? "#/pair?name=oops" : ["code", "key", "unknown"].includes(mode) ? "#/pair" : `#/pair?principal=${principal}&name=test-box`;
await page.goto(`${origin}/${hash}`);
if (mode === "signin") {
  await page.waitForSelector('form[action="/login/social"] button', { timeout: 30000 });
  ok(new URL(page.url()).pathname === "/login", `${tag}: no session: the sign-in (${page.url()})`);
  await shot("signin");
  await page.click('form[action="/login/social"] button');
  await page.waitForURL((v) => v.pathname === "/" && v.hash.startsWith("#/pair"), { timeout: 30000 }).catch(() => {});
  ok((await page.evaluate(() => location.pathname + location.hash)) === `/${hash}`, `${tag}: the sign-in came back to #/pair with the query whole (${page.url()})`);
  // Signed in just now, they hold nothing: the operator makes them admin of KILN, as one would be.
  execSync("bash -c promote", { encoding: "utf8" });
  await page.reload();
}
await page.waitForSelector("#pair-page", { timeout: 30000 }).catch(async (e) => {
  console.log("hash:", await page.evaluate(() => location.hash), "no #pair-page; the page says:", (await page.innerText("main").catch(() => "")).slice(0, 800));
  throw e;
});
await page.waitForFunction(() => !document.getElementById("boot"), null, { timeout: 30000 });

if (mode === "bad") {
  ok(await page.isVisible("#pair-bad"), `${tag}: a link with no key says so`);
  ok(!(await page.$("#pair-go")) && !(await page.$("#pair-code")), `${tag}: and offers nothing`);
  await shot("bad");
} else {
  if (["code", "key", "unknown"].includes(mode)) {
    await page.waitForSelector("#pair-text", { timeout: 15000 });
    ok(/Enter the code your terminal shows, or paste its key/.test(await page.textContent("#pair-entry")), `${tag}: the field asks for the code or the key`);
    await safeArea(["#pair-text", "#pair-find"], "the field");
    const typedIn = mode === "code" ? code(machine).toLowerCase().replace("-", "") : mode === "key" ? machine.toUpperCase() : (code(machine) === "AAAA-AAAA" ? "bbbb bbbb" : "aaaa aaaa");
    await page.fill("#pair-text", typedIn);
    await shot("typed");
    await page.click("#pair-find");
    if (mode === "unknown") {
      await page.waitForSelector("#pair-note", { timeout: 15000 });
      ok(/No machine waiting shows/.test(await page.textContent("#pair-note")), `${tag}: an unknown code is refused in band (${await page.textContent("#pair-note")})`);
      ok(!(await page.$("#pair-go")), `${tag}: and offers nothing to pair`);
      await shot("unknown");
    }
  }
}
if (mode !== "bad" && mode !== "unknown") {
  await page.waitForSelector("#pair-code", { timeout: 15000 });
  ok((await page.textContent("#pair-code")) === code(machine), `${tag}: the page shows the code tether prints (${await page.textContent("#pair-code")})`);
  if (mode === "code") ok(!(await page.$("#pair-ask")) && /You typed the code/.test(await page.textContent("#pair-matched")), `${tag}: a typed code is the match, not asked again`);
  else ok(new RegExp(`Does your terminal show ${code(machine)}\\?`).test(await page.textContent("#pair-ask")), `${tag}: it asks whether the terminal shows the code`);
  ok((await page.textContent("#pair-principal")) === principal && (await page.textContent("#pair-name")) === "test-box", `${tag}: the machine's name and principal`);
  if (mode === "bob") {
    await page.waitForSelector("#pair-cannot", { timeout: 15000 });
    ok(/only an admin can/.test(await page.textContent("#pair-cannot")), `${tag}: a viewer is told who can`);
    ok(!(await page.$("#pair-go")), `${tag}: and has no Pair`);
    await shot("cannot");
  } else {
    await page.waitForSelector("#pair-go", { timeout: 15000 });
    const offered = await page.$$eval("#pair-scope option", (os) => os.map((o) => o.value));
    ok(offered.join() === "hypermemetic:KILN", `${tag}: offered only where alice is admin (${offered.join()})`);
    ok((await page.$eval("#pair-role", (s) => s.value)) === "editor", `${tag}: editor by default`);
    const inside = await page.$$eval("#pair-go, #pair-not-mine", (bs) => bs.every((b) => { const r = b.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth && r.height >= 40; }));
    ok(inside, `${tag}: Pair and No inside the viewport, tall enough to tap`);
    ok((await page.textContent("#pair-go")) === (mode === "code" ? "Pair it" : "Yes, pair it") && (await page.textContent("#pair-not-mine")) === "No", `${tag}: ${await page.textContent("#pair-go")} / No`);
    await safeArea(["#pair-code", "#pair-role", "#pair-scope", "#pair-go", "#pair-not-mine"], "the confirm screen");
    await shot("open");
    if (mode !== "notmine") {
      await page.click("#pair-go");
      await page.waitForSelector("#pair-done", { timeout: 15000 });
      ok(/test-box is editor on hypermemetic:KILN/.test(await page.textContent("#pair-done")), `${tag}: Pair says what it granted (${await page.textContent("#pair-done")})`);
      const g = grantsOf();
      ok(g.length === 1 && g[0].includes('"editor"') && g[0].includes("hypermemetic:KILN"), `${tag}: the grant is kept (${g.join(" ")})`);
      ok(!(await page.$("#pair-go")), `${tag}: Pair is gone once paired`);
      await shot("paired");
    } else {
      await page.click("#pair-not-mine");
      await page.waitForSelector("#pair-dismissed", { timeout: 15000 });
      ok(/If someone sent you this link/.test(await page.textContent("#pair-dismissed")), `${tag}: No says what to do if someone sent the link`);
      ok(!(await page.$("#pair-go")), `${tag}: Not mine dismisses it`);
      ok(grantsOf().length === 0, `${tag}: and grants nothing`);
      await shot("dismissed");
    }
  }
}
const over = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
ok(over <= 0, `${tag}: nothing wider than the page (${over} px over)`);
ok(errs.length === 0, `${tag}: no page errors${errs.length ? ` (${errs.join("; ")})` : ""}`);
await browser.close();
console.log(fails.length ? `${fails.length} failed` : "all ok");
process.exit(fails.length ? 1 : 0);
