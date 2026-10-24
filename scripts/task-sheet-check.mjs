#!/usr/bin/env node
// TICKETS-23: a task's row opens a sheet that is a full ticket body, drawn by
// the ticket page's own renderer — its markdown and code in a /app/frame
// frame, its ```mockup drawn by the host, its m-decision answerable and
// recorded as the ticket's input; Esc and the scrim close it; ?task= reopens
// it; ‹ › step through the body's tasks; a task inside the sheet folds in
// place (one level); no page errors, no horizontal scroll. Against a
// throwaway (scripts/task-sheet-check.sh), whose KILN-3 holds the tasks.
//
//   node scripts/task-sheet-check.mjs ORIGIN SESSION_FILE WIDTH THEME [SHOTS]
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
const tag = `${width}-${scheme}`;
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${tag}: ${what}`); };
const T = "KILN-3";

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ viewport: { width, height: phone ? 844 : 900 }, colorScheme: scheme, hasTouch: phone });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const logs = [];
page.on("console", (m) => { if (m.type() === "error" || m.type() === "warning") logs.push(m.text()); });
const shot = async (name) => { if (shots) await page.screenshot({ path: `${shots}/${tag}-${name}.png` }); };
const hash = () => page.evaluate(() => location.hash);
const sheetOpen = () => page.locator("#tsheet").isVisible().catch(() => false);
const live = (sel) => page.waitForFunction((s) => [...document.querySelectorAll(s)].some((f) => f.getAttribute("data-state") !== "pending"), sel, { timeout: 30000 });
const overflow = () => page.evaluate(() => ({ sw: document.scrollingElement.scrollWidth, iw: innerWidth }));
// A task's own head: its row's, not a task's inside it.
const head = (id) => `m-task#${id} .h:has(code.id:text-is("${id}"))`;
const body = page.frameLocator("#ticket-page iframe.mframe-doc").first();
const inSheet = page.frameLocator("#tsheet iframe.mframe-doc").first();

async function done() {
  ok(errs.length === 0, `no page errors${errs.length ? ` (${errs.join(" | ")})` : ""}`);
  await browser.close();
  process.exit(fails.length ? 1 : 0);
}

await page.goto(`${origin}/app/#/ticket/${T}`);
try {
  await live("#ticket-page iframe.mframe-doc");
  await body.locator(head("glaze")).waitFor({ timeout: 30000 });
} catch (e) {
  await shot("none");
  ok(false, `the body's task rows drew (${String(e).split("\n")[0]} ${logs.slice(-5).join(" | ")})`);
  await done();
}
await page.waitForTimeout(500);
await shot("page");

// 1. The row opens the sheet; the route says which task.
await body.locator(head("glaze")).click();
await page.locator("#tsheet").waitFor({ timeout: 10000 }).catch(() => {});
ok(await sheetOpen(), "the row opens the sheet");
ok((await hash()).includes("task=glaze"), `the route names the task (${await hash()})`);
ok(await page.evaluate(() => !document.querySelector("#ticket-page #tsheet")), "the sheet sits outside #ticket-page");
ok((await page.locator("#tsheet .tstitle").innerText()).includes("Glaze the test tiles"), "its head says the task's title");
ok((await page.locator("#tsheet .tsp").count()) === 3, "its parameters as chips (files, model, verify)");

// 2. The same renderer: a /app/frame frame, never a srcdoc, stamped with the ticket.
await live("#tsheet iframe.mframe-doc").catch(() => {});
const fr = await page.evaluate(() => {
  const f = document.querySelector("#tsheet iframe.mframe-doc");
  return f && { src: f.getAttribute("src") || "", srcdoc: f.hasAttribute("srcdoc"), ticket: f.getAttribute("data-ticket") };
});
ok(fr && fr.src.includes("/app/frame") && !fr.srcdoc, `the sheet's body is a /app/frame frame, no srcdoc (${fr && fr.src})`);
ok(fr && fr.ticket === T, `the frame is the ticket's (data-ticket=${fr && fr.ticket})`);
await inSheet.locator("h2").first().waitFor({ timeout: 15000 }).catch(() => {});
ok((await inSheet.locator("h2").first().innerText().catch(() => "")).includes("The brief"), "markdown: the heading");
ok((await inSheet.locator("strong").first().innerText().catch(() => "")).includes("test tiles"), "markdown: the bold");
ok((await inSheet.locator("pre code").first().innerText().catch(() => "")).includes("fn fire"), "the code block");

// 3. The mockup is drawn by the host, over the sheet's frame, as on a page.
await page.locator("#tsheet .mmock").first().waitFor({ timeout: 15000 }).catch(() => {});
const mocks = await page.locator("#tsheet .mmock").count();
ok(mocks >= 1, `the host draws the mockup in the sheet (${mocks} box)`);
const devices = await page.locator("#tsheet .mmock iframe").count();
ok(devices >= 1, `the mockup's device frames (${devices})`);

// 4. The task inside folds in place: one level of sheets.
const inner = inSheet.locator(head("glaze-note"));
await inner.scrollIntoViewIfNeeded().catch(() => {});
await inner.click().catch((e) => ok(false, `click the inner task: ${e.message}`));
await page.waitForTimeout(300);
ok((await hash()).includes("task=glaze") && !(await hash()).includes("task=glaze-note"), "a task inside the sheet folds in place, the sheet stays");
ok(await inSheet.locator(`${head("glaze-note")}[aria-expanded=true]`).count() === 1, "the inner task is folded open");

// 5. The decision answers, as the ticket's input.
const opt = inSheet.locator('m-decision#cone button[data-value="6"]');
await opt.scrollIntoViewIfNeeded().catch(() => {});
await opt.click({ timeout: 10000 }).catch((e) => ok(false, `click the option: ${e.message.split("\n").slice(0, 12).join(" / ")}`));
await inSheet.locator("m-decision#cone .a").filter({ hasText: /answered|refused/ }).waitFor({ timeout: 10000 }).catch(() => {});
const said = await inSheet.locator("m-decision#cone .a").innerText().catch(() => "");
ok(said === "answered: 6", `the decision is answered in the sheet (${said})`);
let o = await overflow();
ok(o.sw <= o.iw, `the sheet open, no horizontal scroll (${o.sw} <= ${o.iw})`);
ok(await page.evaluate(() => document.documentElement.hasAttribute("data-sheet-lock")), "the page is locked under the sheet");
const box = await page.locator("#tsheet").boundingBox();
ok(box && box.x >= -0.5 && box.x + box.width <= width + 0.5, `the sheet inside the viewport (${box && Math.round(box.x)}+${box && Math.round(box.width)})`);
if (phone) ok(box && box.y > 0 && Math.abs(box.y + box.height - 844) < 2, "a bottom sheet on a phone");
else ok(box && Math.abs(box.x + box.width - width) < 2 && box.height >= 890, "a side sheet at 1440");
await inSheet.locator("h2").first().scrollIntoViewIfNeeded().catch(() => {});
await page.waitForTimeout(300);
await shot("sheet");
await page.locator("#tsheet .mmock").first().scrollIntoViewIfNeeded().catch(() => {});
await page.waitForTimeout(300);
await shot("sheet-mockup");

// 6. Stepping: ‹ › through the body's tasks, in document order.
const next = page.locator('#tsheet .tsstep[aria-label="Next task"]');
await next.click();
await page.waitForTimeout(200);
ok((await hash()).includes("task=glaze-note") && (await page.locator("#tsheet .tsplace").innerText()).includes("2 of 3"), "› steps to the next task (2 of 3)");
await next.click();
await page.waitForTimeout(200);
ok((await hash()).includes("task=stack") && (await next.isDisabled()), "› to the last, then stops");
// A body of plain markdown draws in place, unframed, as on the page.
await page.locator("#tsheet .tsbody").filter({ hasText: "heaviest at the bottom" }).waitFor({ timeout: 10000 }).catch(() => {});
const stepped = await page.locator("#tsheet .tsbody").innerText().catch((e) => String(e));
ok(stepped.includes("heaviest at the bottom"), `the stepped-to task's body draws${stepped.includes("heaviest") ? "" : ` (${stepped.slice(0, 120)})`}`);
await shot("stepped");
await page.locator('#tsheet .tsstep[aria-label="Previous task"]').click();
await page.locator('#tsheet .tsstep[aria-label="Previous task"]').click();
await page.waitForTimeout(200);
ok((await hash()).includes("task=glaze") && !(await hash()).includes("glaze-note"), "‹ steps back to the first");

// 7. Esc closes it; the route forgets the task.
await page.locator("#tsheet .tsx").focus();
await page.keyboard.press("Escape");
await page.waitForTimeout(300);
ok(!(await sheetOpen()) && !(await hash()).includes("task="), "Esc closes the sheet");
ok(await page.evaluate(() => !document.documentElement.hasAttribute("data-sheet-lock")), "the page unlocks");

// 8. The scrim closes it.
await body.locator(head("stack")).click();
await page.locator("#tsheet").waitFor({ timeout: 10000 }).catch(() => {});
ok(await sheetOpen() && (await hash()).includes("task=stack"), "another row opens its own task");
if (phone) await page.mouse.click(width / 2, 30); else await page.mouse.click(40, 450);
await page.waitForTimeout(300);
ok(!(await sheetOpen()), "the scrim closes the sheet");

// 9. The deep link reopens it, on a fresh load.
await page.goto("about:blank");
await page.goto(`${origin}/app/#/ticket/${T}?task=glaze`);
await page.locator("#tsheet").waitFor({ timeout: 30000 }).catch(() => {});
ok(await sheetOpen(), "?task=glaze reopens the sheet on a fresh load");
await inSheet.locator("h2").first().waitFor({ timeout: 15000 }).catch(() => {});
ok((await inSheet.locator("h2").first().innerText().catch(() => "")).includes("The brief"), "… with the task's body");
o = await overflow();
ok(o.sw <= o.iw, `no horizontal scroll (${o.sw} <= ${o.iw})`);
await page.goto("about:blank");
await page.goto(`${origin}/app/#/ticket/${T}?task=nope`);
await page.waitForTimeout(2500);
ok(await sheetOpen() && (await page.locator("#tsheet .tstitle.missing").count()) === 1, "a task the body lacks says so");
await done();
