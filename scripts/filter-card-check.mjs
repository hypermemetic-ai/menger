#!/usr/bin/env node
// FILTER-53: the card on a saved chip — hover opens it (a press pins it; on a
// phone a sheet with Rules / The nav tabs), a name inside opens in place, a
// rule and a group are added, a group's joiner flips, a rule negates, a field
// changes, a part goes in the bin, a group is named into a saved filter (a
// taken name refused), save back and undo, the switch takes it off the board
// and back, the nav under it is a tree or flat, the empty line's offers and
// ⌂'s chip open cards. Against a throwaway (scripts/filter-card-check.sh);
// screenshots when SHOTS is given.
//
//   node scripts/filter-card-check.mjs ORIGIN SESSION_FILE WIDTH THEME [SHOTS]
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
const settle = (ms = 500) => page.waitForTimeout(ms);
const top = "Plexus-Attention";
const go = async (q) => { await page.goto(`${origin}/app/#/${q}`); await settle(1200); };
await page.goto(`${origin}/app/#/?f=${encodeURIComponent(`saved "${top}"`)}`);
try {
  await page.waitForSelector('.fline [data-fbtn="board"]', { timeout: 30000 });
  await page.waitForFunction(() => /live/.test(document.querySelector("#conn")?.textContent ?? "live"), null, { timeout: 15000 });
  await page.waitForSelector(`.fline button.fchip-b[data-fcard="${top}"]`, { timeout: 15000 });
} catch {
  await shot("none");
  console.log(`FAIL ${tag}: the saved chip did not draw (${errs.join(" | ")})`);
  process.exit(1);
}
await page.evaluate(() => document.fonts.ready);
await settle(800);

const card = ".fcard";
const chip = `.fline button.fchip-b[data-fcard="${top}"]`;
const isOpen = (name) => page.locator(name ? `${card}[data-fcard-open="${name}"]` : card).count().then((n) => n > 0);
const rules = () => page.locator(`${card} .fc-rule`).count();
const edited = () => page.locator(`${card} .fc-edrow`).count();
const fits = async (what) => {
  const r = await page.evaluate(() => {
    const b = document.querySelector(".fcard")?.getBoundingClientRect();
    return { b: b?.bottom ?? 0, t: b?.top ?? 0, l: b?.left ?? 0, r: b?.right ?? 0, h: innerHeight, w: innerWidth, sw: document.scrollingElement.scrollWidth };
  });
  ok(r.t >= 0 && r.b <= r.h + 1 && r.l >= 0 && r.r <= r.w + 1, `${tag}: ${what} inside the viewport (${Math.round(r.l)},${Math.round(r.t)} → ${Math.round(r.r)},${Math.round(r.b)} in ${r.w}×${r.h})`);
  ok(r.sw <= r.w, `${tag}: ${what}: no horizontal scroll (${r.sw} <= ${r.w})`);
};
const undoAll = async () => { while (await page.locator(`${card} [data-fc="undo"]`).count()) { await page.locator(`${card} [data-fc="undo"]`).first().click(); await settle(300); } };
const pickFirst = async (cat) => {
  await page.waitForSelector(`${card} .fc-pk`);
  await page.click(`${card} .fc-pkc[data-pkcat="${cat}"]`);
  await settle(300);
  const v = page.locator(`${card} .fc-pkv[data-pkv]`).first();
  const text = await v.getAttribute("data-pkv");
  await v.click();
  await settle(500);
  return text;
};

// Opening: hover (not on a phone) is a peek, a press pins it.
if (!phone) {
  await page.hover(chip);
  await settle(700);
  ok(await isOpen(top), `${tag}: hovering the chip opens its card`);
  await page.mouse.move(width - 20, 880);
  await settle(700);
  ok(!(await isOpen()), `${tag}: leaving it closes the peek`);
}
await page.click(chip);
await settle(900);
ok(await isOpen(top), `${tag}: a press opens the card`);
if (phone) {
  ok(await page.locator(`${card}.sheet`).count() === 1, `${tag}: on a phone the card is a sheet`);
  ok(await page.locator(`${card} button[data-tab="rules"]`).count() === 1 && await page.locator(`${card} button[data-tab="nav"]`).count() === 1, `${tag}: Rules and The nav as tabs`);
}
await fits("the card");
const n0 = await rules();
ok(n0 >= 4, `${tag}: its rules drawn as builders (${n0})`);
ok(await page.locator(`${card} .fc-rule.named[data-named="Plexus-Ready"]`).count() === 1, `${tag}: a name inside is a row of its own`);
ok(await page.locator(`${card} [data-fc="flip"]`).count() >= 2, `${tag}: each group's joiner is a pill`);
await shot("01-card");

// A name opened in place, its parts read-only beside the holder's.
await page.click(`${card} .fc-rule.named[data-named="Plexus-Ready"] [data-fc="unfold"]`);
await settle(800);
const inner = await page.locator(`${card} .fc-sbox[data-sbox="Plexus-Ready"] .fc-rule`).count();
ok(inner >= 2, `${tag}: Plexus-Ready opens in place (${inner} parts)`);
await shot("02-unfolded");
await page.click(`${card} .fc-rule.named[data-named="Plexus-Ready"] [data-fc="unfold"]`);
await settle(300);

// Negate a rule: a draft, counted; undo takes it back.
const kind = `${card} .fc-rule[data-rule="kind gap"]`;
await page.click(`${kind} [data-fc="neg"]`);
await settle(700);
ok(await page.locator(`${card} .fc-rule[data-rule="not kind gap"]`).count() === 1, `${tag}: is ⇄ is not negates the rule`);
ok(await edited() === 1, `${tag}: the change is a draft, shown edited`);
await undoAll();
ok(await page.locator(kind).count() === 1 && await edited() === 0, `${tag}: undo takes it back`);

// Flip a group's joiner.
const pills = await page.locator(`${card} [data-fc="flip"]`).allInnerTexts();
await page.locator(`${card} [data-fc="flip"]`).nth(1).click();
await settle(600);
const flipped = await page.locator(`${card} [data-fc="flip"]`).allInnerTexts();
ok(flipped[1] !== pills[1], `${tag}: the pill flips and ⇄ or (${pills[1].trim()} → ${flipped[1].trim()})`);
await undoAll();

// Add a rule through the picker, then bin it.
await page.click(`${card} [data-fc="add-rule"]`);
const added = await pickFirst("Status");
ok(await rules() === n0 + 1, `${tag}: Add rule adds ${added} (${n0} → ${await rules()})`);
await shot("03-added");
await page.locator(`${card} .fc-rule[data-rule="${added}"] [data-fc="bin"]`).click();
await settle(600);
ok(await rules() === n0 && await edited() === 0, `${tag}: the bin takes it out (and the draft with it)`);

// Add a group.
const g0 = await page.locator(`${card} .fc-g`).count();
await page.click(`${card} [data-fc="add-group"]`);
await pickFirst("Kind");
ok(await page.locator(`${card} .fc-g`).count() === g0 + 1, `${tag}: Add group adds a group`);
await undoAll();

// Change a rule's field.
await page.click(`${kind} [data-fc="field"]`);
const swapped = await pickFirst("Severity");
ok(await page.locator(kind).count() === 0 && await page.locator(`${card} .fc-rule[data-rule="${swapped}"]`).count() >= 1, `${tag}: changing the field replaces the rule (kind gap → ${swapped})`);
await undoAll();

// FILTER-54: a rule's time — ◷ at a status or kind value's end opens Became
// so… (a status also Has been so…), each choice counted; a set time is its
// own line (change it, × takes it off); Created / Changed are fields whose
// value is the time; a date of the reader's own; as text says the new form.
const tm = `${card} .fc-tm`;
await page.click(`${kind} [data-fc="time-add"]`);
await page.waitForSelector(tm);
await settle(600);
const heads = await page.locator(`${tm} .fc-tmh`).allInnerTexts();
ok(heads.length === 1 && /Became so/.test(heads[0]), `${tag}: a kind's ◷ opens Became so… alone (${heads.join(" / ")})`);
const counted = await page.locator(`${tm} .fc-tmi .fc-pkn`).allInnerTexts();
ok(counted.some((t) => /\d/.test(t)), `${tag}: each time counted (${counted.slice(0, 3).join(",")})`);
await shot("08-time-menu");
await page.keyboard.press("Escape");
await settle(300);
ok(await page.locator(tm).count() === 0 && await isOpen(top), `${tag}: Escape closes the time menu, the card stays`);
await page.click(`${kind} [data-fc="time-add"]`);
await page.click(`${tm} .fc-tmi[data-tm="within 3d"]`);
await settle(700);
const timed = `${card} .fc-rule[data-rule="kind gap within 3d"]`;
ok(await page.locator(timed).count() === 1, `${tag}: within 3d is the rule's time`);
ok(/became so/.test(await page.locator(`${timed} .fc-trow`).innerText().catch(() => "")), `${tag}: said on its own line, became so`);
ok(await edited() === 1, `${tag}: a draft, counted`);
await page.click(`${card} [data-fc="as-text"]`);
await settle(300);
ok(/kind gap within 3d/.test(await page.locator(`${card} .fc-txt`).innerText().catch(() => "")), `${tag}: as text says the new form`);
await page.click(`${card} [data-fc="as-text"]`);
await page.click(`${timed} [data-fc="time"]`);
await page.click(`${tm} .fc-tmi[data-tm="today"]`);
await settle(600);
ok(await page.locator(`${card} .fc-rule[data-rule="kind gap today"]`).count() === 1, `${tag}: the time changes`);
await shot("09-timed");
await page.click(`${card} .fc-rule[data-rule="kind gap today"] [data-fc="time-off"]`);
await settle(600);
ok(await page.locator(kind).count() === 1 && await edited() === 0, `${tag}: × takes the time off (the draft with it)`);
// A date of the reader's own.
await page.click(`${kind} [data-fc="time-add"]`);
await page.waitForSelector(tm);
await page.selectOption(`${tm} .fc-tcus select`, "before");
await page.fill(`${tm} .fc-tcus input[type="date"]`, "2026-10-01");
await page.click(`${tm} [data-fc="time-own"]`);
await settle(600);
ok(await page.locator(`${card} .fc-rule[data-rule="kind gap before 2026-10-01"]`).count() === 1, `${tag}: a date of one's own`);
await undoAll();
// A status: has been so.
await page.click(`${card} .fc-rule.named[data-named="Plexus-Ready"] [data-fc="unfold"]`);
await settle(800);
const st = `${card} .fc-sbox[data-sbox="Plexus-Ready"] .fc-rule[data-rule="not status Complete"]`;
await page.click(`${st} [data-fc="time-add"]`);
await page.waitForSelector(tm);
ok((await page.locator(`${tm} .fc-tmh`).allInnerTexts()).some((t) => /Has been so/.test(t)), `${tag}: a status's ◷ also offers Has been so…`);
await page.click(`${tm} .fc-tmi[data-tm="for over 1w"]`);
await settle(700);
const held = `${card} .fc-rule[data-rule="not status Complete for over 1w"]`;
ok(await page.locator(held).count() === 1 && /has been so/.test(await page.locator(`${held} .fc-trow`).innerText().catch(() => "")), `${tag}: has been so, for over 1w`);
ok(await page.locator(`${card} .fc-edrow[data-edited="Plexus-Ready"]`).count() === 1, `${tag}: an edit of the name it lands in`);
await undoAll();
await page.click(`${card} .fc-rule.named[data-named="Plexus-Ready"] [data-fc="unfold"]`);
await settle(300);
// Created: a field whose value is the time.
await page.click(`${card} [data-fc="add-rule"]`);
const date = await pickFirst("Dates");
ok(date === "created this week" && await page.locator(`${card} .fc-rule[data-rule="created this week"] [data-fc="time"]`).count() === 1, `${tag}: Dates adds Created, its value the time (${date})`);
await page.click(`${card} .fc-rule[data-rule="created this week"] [data-fc="time"]`);
await page.waitForSelector(tm);
const dateHead = await page.locator(`${tm} .fc-tmh`).first().innerText();
ok(/Created/.test(dateHead) && await page.locator(`${tm} .fc-tmi[data-tm="any time"]`).count() === 0, `${tag}: its menu is Created…, no any time`);
await page.click(`${tm} .fc-tmi[data-tm="today"]`);
await settle(600);
ok(await page.locator(`${card} .fc-rule[data-rule="created today"]`).count() === 1, `${tag}: its window changes`);
await undoAll();

// Name a group: a taken name is refused, a free one saves and the holder names it.
await page.locator(`${card} [data-fc="name-group"]`).first().click();
await settle(300);
await page.fill(`${card} .fc-nest .fc-gin`, "Needs you");
await page.click(`${card} [data-fc="name-ok"]`);
await settle(500);
const refused = await page.locator(`${card} .fc-note.bad`).innerText().catch(() => "");
ok(/already/.test(refused), `${tag}: a taken name is refused (${refused.trim()})`);
const fresh = `Attention-${tag}`;
await page.fill(`${card} .fc-nest .fc-gin`, fresh);
await page.click(`${card} [data-fc="name-ok"]`);
await settle(1200);
ok(await page.locator(`${card} .fc-rule.named[data-named="${fresh}"]`).count() === 1, `${tag}: the named group is a saved filter, named in its place`);
await shot("04-named");
await undoAll();

// Save back, then back again (a negation: flipping a joiner and saving it
// merges groups that share one, so it is not its own inverse).
await page.click(`${kind} [data-fc="neg"]`);
await settle(500);
await page.click(`${card} [data-fc="save-back"]`);
await settle(1200);
ok(await edited() === 0 && await page.locator(`${card} .fc-rule[data-rule="not kind gap"]`).count() === 1, `${tag}: save back keeps it (no draft left)`);
await page.click(`${card} .fc-rule[data-rule="not kind gap"] [data-fc="neg"]`);
await settle(500);
await page.click(`${card} [data-fc="save-back"]`);
await settle(1200);
ok(await edited() === 0 && await page.locator(kind).count() === 1, `${tag}: and saved back as it was`);

// The switch: off the board, then on again.
await page.click(`${card} [data-fc-switch]`);
await settle(800);
ok(await page.locator(chip).count() === 0, `${tag}: the switch takes it off the board`);
await page.click(`${card} [data-fc-switch]`);
await settle(800);
ok(await page.locator(chip).count() === 1, `${tag}: and puts it back`);

// The nav under it: tree, a ticket unfolded, flat.
if (phone) { await page.click(`${card} button[data-tab="nav"]`); await settle(500); }
await page.waitForSelector(`${card} .fc-nr[data-nrow]`, { timeout: 10000 }).catch(() => {});
const t0 = await page.locator(`${card} .fc-nr[data-nrow]`).count();
ok(t0 >= 1, `${tag}: the nav under it draws a tree (${t0} rows)`);
const fold = page.locator(`${card} .fc-nt[data-nav][aria-expanded="false"]`).first();
if (await fold.count()) {
  await fold.click();
  await settle(900);
  ok(await page.locator(`${card} .fc-nr[data-nrow]`).count() > t0, `${tag}: a ticket unfolds in it`);
}
await page.click(`${card} [data-flat="1"]`);
await settle(900);
const flat = await page.locator(`${card} .fc-nr[data-nrow]`).allInnerTexts();
ok(flat.length >= 1 && (await page.locator(`${card} .fc-nr.hold`).count()) === 0, `${tag}: flat lists the matches alone (${flat.length})`);
await shot("05-nav");
await page.click(`${card} [data-flat="0"]`);
if (phone) await page.click(`${card} button[data-tab="rules"]`);

// Escape closes it.
await page.keyboard.press("Escape");
await settle(400);
ok(!(await isOpen()), `${tag}: Escape closes it`);

// The empty line: its offers open a card, + add a filter the menu; ⌂'s chip.
await go(`?g=${encodeURIComponent("not kind doc")}&f=`);
const offer = page.locator(".fline .fchip-sug[data-fcard]").first();
ok(await offer.count() === 1, `${tag}: the empty line offers saved filters`);
const offered = await offer.getAttribute("data-fcard");
await offer.click();
await settle(900);
ok(await isOpen(offered), `${tag}: an offer opens its card (${offered})`);
await shot("06-offer");
await page.keyboard.press("Escape");
await settle(400);
await page.locator('.fline .fchip-g[data-fcard="⌂"]').first().click();
await settle(900);
ok(await isOpen("⌂") && (await page.getAttribute(card, "data-ro")) === "true", `${tag}: ⌂'s chip opens its card, read-only here`);
await fits("⌂'s card");
await shot("07-home");
await page.keyboard.press("Escape");
await settle(400);
await page.click(".fline [data-first]");
await settle(500);
ok(await page.locator(".fmenu").count() === 1, `${tag}: + add a filter opens the menu`);

ok(await page.locator('.fmenu .frow.cat[data-sec="when"]').count() === 0 && await page.locator(".fmenu .frow.cat").count() > 0, `${tag}: the menu has no When category`);

ok(errs.length === 0, `${tag}: no page errors (${errs.join(" | ")})`);
await browser.close();
if (fails.length) { console.log(`${fails.length} failed`); process.exit(1); }
