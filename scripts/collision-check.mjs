#!/usr/bin/env node
// TICKETS-36: the app's links by uid and the collision page, in Chromium,
// against a throwaway (scripts/collision-check.sh) holding two shortcodes
// named PERF: MENGER/STORE's PERF-2 (uid OUID) and PLEXUS's (uid NUID).
//
//   - #/ticket/PERF-2 is the collision page: two rows in the store's order,
//     each its address (hosts, id), kind, status, title and parent — a link
//     by uid; the first chosen; no horizontal scroll.
//   - ↓ ↑ j k choose; c copies the chosen link (/t/<uid>/PERF-2-…); ⏎ opens
//     it at #/t/<uid>/PERF-2-… and the page draws; a click on the other row
//     opens it; Esc goes to the board.
//   - Copy link on the ticket's page gives <origin>/t/<uid>/<ID>-<slug>.
//   - #/ticket/<full address> is replaced by its #/t/… (no history entry);
//     #/ticket/NOPE-1 is not found; a stale slug is rewritten in place, cold
//     and warm.
//   - The board's row for the original PERF-2 links by its uid and opens it;
//     the palette's row for it opens it at its uid.
//   - The shell: /PERF-2 lands on the collision page, /t/<uid>/x on the page.
//   - A reader of MENGER's tree alone, asking for PERF-2, is taken to the
//     one ticket the name fits for them.
//
//   node scripts/collision-check.mjs ORIGIN OP_SESSION BOB_SESSION OUID NUID WIDTH THEME [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, opFile, bobFile, ouid, nuid, widthArg, scheme, shots] = process.argv.slice(2);
const width = Number(widthArg);
const phone = width <= 820;
const port = new URL(origin).port;
const cookie = (file) => ({ name: `menger_session_${port}`, value: fs.readFileSync(file, "utf8").trim(), url: `${origin}/`, httpOnly: true, sameSite: "Lax" });
const fails = [];
const tag = `${width}-${scheme}`;
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${tag}: ${what}`); };
const ORIG = `#/t/${ouid}/PERF-2-compression-on-the-websocket`;
const NAMESAKE = `#/t/${nuid}/PERF-2-the-schema-answers-3-24-mb`;

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width, height: phone ? 844 : 900 }, colorScheme: scheme, hasTouch: phone });
await ctx.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
await ctx.addCookies([cookie(opFile)]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const shot = async (name) => { if (shots) await page.screenshot({ path: `${shots}/${tag}-${name}.png` }); };
const hash = () => page.evaluate(() => location.hash);
const hashIs = (h, t = 15000) => page.waitForFunction((want) => location.hash === want, h, { timeout: t }).then(() => true, () => false);
const title = () => page.locator("#ticket-page h2 .t").first().innerText({ timeout: 15000 }).catch(() => "");
const clip = () => page.evaluate(() => navigator.clipboard.readText()).catch(() => "");
const overflow = () => page.evaluate(() => document.scrollingElement.scrollWidth - innerWidth);
const go = async (route) => {
  await page.evaluate((r) => { location.hash = r; }, route);
};
const live = () => page.waitForFunction(() => /live/.test(document.querySelector("#conn")?.textContent || ""), null, { timeout: 30000 });

async function done() {
  ok(errs.length === 0, `no page errors${errs.length ? ` (${errs.join(" | ")})` : ""}`);
  await browser.close();
  process.exit(fails.length ? 1 : 0);
}

// 1. The collision page.
await page.goto(`${origin}/app/#/ticket/PERF-2`);
try {
  await live();
  await page.locator("#collision .choice").nth(1).waitFor({ timeout: 30000 });
} catch (e) {
  await shot("none");
  ok(false, `#/ticket/PERF-2 drew the collision page (${String(e).split("\n")[0]})`);
  await done();
}
await page.evaluate(() => document.fonts.ready);
// DESIGN-39: the boot's sheet removes itself after the first health.
await page.waitForFunction(() => !document.getElementById("boot"), null, { timeout: 15000 }).catch(() => {});
await page.waitForTimeout(300);
await shot("collision");
const rows = await page.$$eval("#collision .choice", (els) => els.map((el) => ({
  addr: el.querySelector(".addr")?.innerText.replace(/\s+/g, " ").trim(),
  hosts: el.querySelector(".hosts")?.innerText.trim(),
  id: el.querySelector(".addr .id")?.innerText.trim(),
  kind: el.querySelector(".kind")?.getAttribute("data-kind"),
  status: el.querySelector(".cst")?.innerText.trim(),
  title: el.querySelector(".ctitle")?.innerText.trim(),
  parent: el.querySelector(".cparent a")?.getAttribute("href"),
  parentId: el.querySelector(".cparent a")?.innerText.trim(),
  parentTitle: el.querySelector(".cparent .pt")?.innerText.trim(),
  link: el.querySelector(".meta .link")?.textContent.trim(),
  selected: el.getAttribute("aria-selected"),
  menu: el.getAttribute("data-ticket-id"),
})));
ok(rows.length === 2, `two rows, one per choice (${rows.length})`);
const [a, b] = rows;
ok(a?.hosts === "hypermemetic : MENGER / STORE /" && a?.id === "PERF-2", `row 1 is MENGER/STORE's PERF-2, hosts muted (${a?.addr})`);
ok(b?.hosts === "hypermemetic : PLEXUS /" && b?.id === "PERF-2", `row 2 is PLEXUS's PERF-2 (${b?.addr})`);
ok(a?.kind === "build" && a?.status === "Ready" && a?.title === "Compression on the WebSocket", `row 1's kind, status, title (${a?.kind} ${a?.status} ${a?.title})`);
ok(b?.kind === "gap" && b?.status === "Ready" && b?.title === "the schema answers 3.24 MB", `row 2's kind, status, title (${b?.kind} ${b?.status} ${b?.title})`);
ok(/^#\/t\/[0-9a-z]{8}\/PERF-1-magnitude-how-menger-degrades$/.test(a?.parent || "") && a?.parentId === "PERF-1" && a?.parentTitle === "Magnitude — how menger degrades", `row 1's parent, linked by uid (${a?.parent})`);
ok(/^#\/t\/[0-9a-z]{8}\/PERF-1-self-description-cached$/.test(b?.parent || "") && b?.parentTitle === "Self-description cached", `row 2's parent, linked by uid (${b?.parent})`);
ok(a?.parent !== b?.parent, "the two parents are two tickets");
ok(a?.link === `/t/${ouid}/PERF-2-compression-on-the-websocket` && b?.link === `/t/${nuid}/PERF-2-the-schema-answers-3-24-mb`, `each row's link is by its uid (${a?.link} · ${b?.link})`);
ok(a?.selected === "true" && b?.selected === "false", "the first row is chosen");
ok(a?.menu === "PERF-2" && b?.menu?.startsWith("PERF.") && b?.menu?.endsWith("-2"), `each row is its ticket for the one menu (${a?.menu}, ${b?.menu})`);
ok((await page.locator("#collision h1").innerText()) === "names two tickets", "the heading says two");
ok((await overflow()) <= 0, `no horizontal scroll (${await overflow()} px over)`);
const linkShown = await page.locator("#collision .choice .meta .link").first().isVisible();
ok(phone ? !linkShown : linkShown, `the link line ${phone ? "is hidden on a phone" : "shows on a desktop"}`);

// 2. The keys.
const chosen = () => page.$$eval("#collision .choice", (els) => els.findIndex((e) => e.getAttribute("aria-selected") === "true"));
await page.locator("#collision h1").click();
await page.keyboard.press("ArrowDown");
ok((await chosen()) === 1, "↓ chooses the second");
ok(await page.evaluate(() => document.activeElement?.id === "choice-1"), "and focuses it");
await page.keyboard.press("ArrowDown");
ok((await chosen()) === 0, "↓ past the last wraps to the first");
await page.keyboard.press("j");
ok((await chosen()) === 1, "j chooses the next");
await page.keyboard.press("k");
ok((await chosen()) === 0, "k the previous");
await page.keyboard.press("c");
await page.waitForFunction(() => /\/t\//.test(document.querySelector("#collision .said")?.textContent || ""), null, { timeout: 5000 }).catch(() => {});
const said = await page.locator("#collision .said").innerText();
ok(said === `copied ${origin}/t/${ouid}/PERF-2-compression-on-the-websocket`, `c copies the chosen link (${said})`);
ok((await clip()) === `${origin}/t/${ouid}/PERF-2-compression-on-the-websocket`, "the clipboard holds it");
await page.keyboard.press("Enter");
ok(await hashIs(ORIG), `⏎ opens it at its uid (${await hash()})`);
ok((await title()) === "Compression on the WebSocket", "and its page draws");
await shot("opened");

// 3. Back to the collision page; a click on the other row.
await page.goBack();
await page.locator("#collision .choice").nth(1).waitFor({ timeout: 15000 }).catch(() => {});
ok((await hash()) === "#/ticket/PERF-2", `Back returns to the collision page (${await hash()})`);
await page.locator("#collision .choice").nth(1).locator(".ctitle").click();
ok(await hashIs(NAMESAKE), `a click opens the other at its uid (${await hash()})`);
ok((await title()) === "the schema answers 3.24 MB", "and its page draws");

// 4. Copy link, on the page.
const copyBtn = page.locator("#ticket-page h2 .copylink");
ok((await copyBtn.count()) === 1, "the page's heading has Copy link");
await copyBtn.click();
await page.waitForFunction(() => document.querySelector("#ticket-page h2 .copylink")?.textContent === "Copied", null, { timeout: 5000 }).catch(() => {});
ok((await copyBtn.innerText()) === "Copied", "it says it copied");
ok((await clip()) === `${origin}/t/${nuid}/PERF-2-the-schema-answers-3-24-mb`, `Copy link is the permalink by uid (${await clip()})`);

// 4b. The palette: the original PERF-2 picked (its key, PERF-2, is the name
// that fits both) opens at its own uid — not the collision page.
await go("#/");
await page.waitForTimeout(200);
await page.locator("body").press("/");
await page.locator("#palette-input").fill("PERF-2");
const palRow = page.locator('#palette-list li[data-pal-id="PERF-2"]');
await palRow.first().waitFor({ timeout: 15000 }).catch(() => {});
await palRow.first().click().catch(() => {});
ok(await hashIs(ORIG), `the palette's row for the original opens it by its uid (${await hash()})`);
if (await page.locator("#palette").isVisible().catch(() => false)) await page.keyboard.press("Escape");

// 5. A full address is replaced by its uid route — no history entry.
await go("#/");
await page.waitForTimeout(200);
const before = await page.evaluate(() => { location.hash = "#/ticket/hypermemetic:MENGER/STORE/PERF-2"; return history.length; });
ok(await hashIs(ORIG), `#/ticket/<full address> becomes its #/t/… (${await hash()})`);
ok((await page.evaluate(() => history.length)) === before, "in place: no history entry");
ok((await title()) === "Compression on the WebSocket", "the address's page draws");

// 6. Not found; a stale slug, warm (the uid known) and cold (a new tab).
await go("#/ticket/NOPE-1");
await page.locator("#notfound").waitFor({ timeout: 15000 }).catch(() => {});
ok(((await page.locator("#notfound h2").innerText().catch(() => "")) || "").includes("NOPE-1 — not found"), "#/ticket/NOPE-1 is not found");
await go("#/t/PERF-2/x");
await page.waitForFunction(() => /PERF-2 — not found|perf-2 — not found/.test(document.querySelector("#notfound h2")?.textContent || ""), null, { timeout: 15000 }).catch(() => {});
ok(/not found/.test(await page.locator("#notfound h2").innerText().catch(() => "")), "#/t/<not a uid> is not found, never read as a name");
await go(`#/t/${ouid}/PERF-2-a-stale-title`);
ok(await hashIs(ORIG), `a stale slug is rewritten (warm: ${await hash()})`);
const cold = await ctx.newPage();
cold.on("pageerror", (e) => errs.push(String(e)));
await cold.goto(`${origin}/app/#/t/${nuid.toUpperCase()}/STORE-9-what-it-was-called`);
ok(await cold.waitForFunction((w) => location.hash === w, NAMESAKE, { timeout: 30000 }).then(() => true, () => false), `a stale slug is rewritten (cold, the uid in capitals: ${await cold.evaluate(() => location.hash)})`);
await cold.close();

// 7. The board's row for the original PERF-2.
if (!phone) {
  await go("#/");
  const row = page.locator('tbody#board tr[data-ticket-id="PERF-2"]');
  await row.waitFor({ timeout: 30000 }).catch(() => {});
  const href = await row.locator("td.id a").getAttribute("href").catch(() => null);
  ok(href === ORIG, `the board's row for the original PERF-2 links by its uid (${href})`);
  await row.locator("td").nth(1).click().catch(() => {});
  ok(await hashIs(ORIG), "and opens it");
  ok((await title()) === "Compression on the WebSocket", "its page draws");
}

// 8. Esc on the collision page: the board.
await go("#/ticket/PERF-2");
await page.locator("#collision .choice").first().waitFor({ timeout: 15000 }).catch(() => {});
await page.locator("#collision h1").click({ timeout: 10000 }).catch(() => {});
await page.keyboard.press("Escape");
ok(await hashIs("#/"), `Esc goes to the board (${await hash()})`);

// 9. The shell: a ticket's name at the root, and a link by uid.
await page.goto(`${origin}/PERF-2`);
await page.locator("#collision .choice").nth(1).waitFor({ timeout: 30000 }).catch(() => {});
ok(new URL(page.url()).pathname === "/app/" && (await hash()) === "#/ticket/PERF-2", `/PERF-2 lands on the collision page (${page.url()})`);
await page.goto(`${origin}/t/${nuid}/x`);
ok(await hashIs(NAMESAKE, 30000), `/t/<uid>/x opens its page, the slug rewritten (${page.url()})`);

// 10. A reader of MENGER's tree alone: the name fits one ticket for them.
const bobCtx = await browser.newContext({ viewport: { width, height: phone ? 844 : 900 }, colorScheme: scheme, hasTouch: phone });
await bobCtx.addCookies([cookie(bobFile)]);
const bob = await bobCtx.newPage();
bob.on("pageerror", (e) => errs.push(String(e)));
await bob.goto(`${origin}/app/#/ticket/PERF-2`);
ok(await bob.waitForFunction((w) => location.hash === w, ORIG, { timeout: 30000 }).then(() => true, () => false), `a reader of one namespace is taken to its ticket (${await bob.evaluate(() => location.hash)})`);
await bobCtx.close();

await done();
