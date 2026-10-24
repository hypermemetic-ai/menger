#!/usr/bin/env node
// FILTER-55: the board's line says only its filter's name (no rules); its
// Filter button and that name open the builder on the board's own filter,
// where every change applies at once; every segment is a dropdown (a field,
// is / is not, values ticked, each counted), a new part a ghost row until
// chosen; More has Save as filter…, Reset; + add a filter opens it on a new
// rule. The nav's head likewise.
// FILTER-53: the card on a saved chip — hover opens it (a press pins it; on a
// phone a sheet with Rules / The nav tabs), a name inside opens in place, a
// rule and a group are added, a group's joiner flips, a rule negates, a field
// changes, a part goes in the bin, a group is named into a saved filter (a
// taken name refused), save back and undo, the switch takes it off the board
// and back, the nav under it is a tree or flat, the empty line's offers open
// cards. FILTER-55 (`now`): ⌂'s chip, F and the phone bar's Filter open the
// builder on ⌂; the wave board's and the inputs' own rules are This page's. Against a throwaway (scripts/filter-card-check.sh);
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
// The empty line offers the saved filters used most recently: this one first.
await page.addInitScript((n) => { try { if (!localStorage.getItem("menger.saved.recent")) localStorage.setItem("menger.saved.recent", n); } catch {} }, top);
await page.goto(`${origin}/app/#/?g=${encodeURIComponent("not kind doc")}&f=`);
try {
  await page.waitForSelector('.fline [data-fbtn="board"]', { timeout: 30000 });
  await page.waitForFunction(() => /live/.test(document.querySelector("#conn")?.textContent ?? "live"), null, { timeout: 15000 });
  await page.waitForSelector(`.fline .fchip-sug[data-fcard="${top}"]`, { timeout: 15000 });
} catch {
  await shot("none");
  console.log(`FAIL ${tag}: the saved chip did not draw (${errs.join(" | ")})`);
  process.exit(1);
}
await page.evaluate(() => document.fonts.ready);
await settle(800);

const card = ".fcard";
const chip = `.fline .fchip-sug[data-fcard="${top}"]`;
const named = `.fline [data-fline-name="board"]`;
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
const dd = `${card} .fc-dd`;
// FILTER-55: a field from the field dropdown, then its first value ticked
// (a date field is the whole rule) — the new rule's text.
const pickFirst = async (field) => {
  await page.waitForSelector(`${dd}[data-dd="field"]`);
  await page.click(`${dd} [data-ddf="${field}"]`);
  await settle(400);
  if (await page.locator(`${dd}[data-dd="values"]`).count()) {
    await page.locator(`${dd} [data-ddv]`).first().click();
    await settle(500);
    await page.click(`${dd} [data-ddclose]:not(.fc-x)`);
  }
  await settle(500);
  return page.locator(`${card} .fc-rule.new`).last().getAttribute("data-rule");
};
const negate = async (rule, to) => {
  await page.click(`${rule} [data-fc="neg"]`);
  await page.waitForSelector(`${dd}[data-dd="op"]`);
  await page.click(`${dd} [data-ddop="${to}"]`);
  await settle(600);
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
// Read only until it is on the board: its switch puts it there.
await page.click(`${card} [data-fc-switch]`);
await settle(1000);
ok(await page.locator(named).innerText().then((t) => t.includes(top)).catch(() => false), `${tag}: the switch puts it on the board, the line says its name`);
ok((await page.getAttribute(card, "data-ro")) === "false", `${tag}: and the card edits it`);
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
await negate(kind, 1);
ok(await page.locator(`${card} .fc-rule[data-rule="not kind gap"]`).count() === 1, `${tag}: is not, from the operator's dropdown, negates the rule`);
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
ok(await page.locator(`${card} .fc-rule.ghost`).count() === 1, `${tag}: a new rule is a ghost row, its field a dropdown`);
await shot("10-dd-field");
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
ok(await page.locator(dd).count() === 0, `${tag}: Done closes the dropdown`);
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
const date = await pickFirst("Created");
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
await negate(kind, 1);
await page.click(`${card} [data-fc="save-back"]`);
await settle(1200);
ok(await edited() === 0 && await page.locator(`${card} .fc-rule[data-rule="not kind gap"]`).count() === 1, `${tag}: save back keeps it (no draft left)`);
await negate(`${card} .fc-rule[data-rule="not kind gap"]`, 0);
await page.click(`${card} [data-fc="save-back"]`);
await settle(1200);
ok(await edited() === 0 && await page.locator(kind).count() === 1, `${tag}: and saved back as it was`);

// The switch: off the board, then on again.
await page.click(`${card} [data-fc-switch]`);
await settle(800);
ok(await page.locator(named).count() === 0, `${tag}: the switch takes it off the board`);
await page.click(`${card} [data-fc-switch]`);
await settle(800);
ok(await page.locator(named).count() === 1, `${tag}: and puts it back`);

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

// FILTER-55: the line says the name and nothing of its rules; the Filter
// button (its badge the rules) and the name open the builder on the board's
// own filter, where a change applies at once.
const line = `${card}[data-fcard-open="line:board"]`;
const badge = () => page.locator('.fline [data-fbtn="board"] .fbadge').innerText().catch(() => "");
ok(await page.locator(".fline .fline-chips .fchip-b, .fline .fline-chips .fchips-g").count() === 0, `${tag}: the line spells out no rules`);
ok(await badge() === "1", `${tag}: the Filter button counts one rule (${await badge()})`);
await page.click('.fline [data-fbtn="board"]');
await settle(900);
ok(await page.locator(line).count() === 1, `${tag}: the Filter button opens the builder on the board's filter`);
ok(/This board's filter/.test(await page.locator(`${line} .fc-head`).innerText()), `${tag}: its head says whose`);
await fits("the builder");
await shot("11-builder");
// (a phone's sheet covers the button: its × closes it)
await page.click(phone ? `${line} .fc-head .fc-x` : '.fline [data-fbtn="board"]');
await settle(500);
ok(!(await isOpen()), `${tag}: ${phone ? "its ×" : "pressed again, it"} closes`);
await page.click(named);
await settle(900);
ok(await page.locator(`${line} .fc-sbox[data-sbox="${top}"]`).count() === 1, `${tag}: the name opens it with ${top} open in place`);
// A rule added: ghost → field → values ticked, the board's filter at once.
await page.click(`${line} [data-fc="add-rule"]`);
await page.waitForSelector(`${dd}[data-dd="field"]`);
ok(await page.locator(`${dd} [data-ddf="Kind"]`).count() === 1 && await page.locator(`${dd} [data-dds]`).count() >= 1, `${tag}: the field dropdown lists fields and saved filters`);
await page.click(`${dd} [data-ddf="Kind"]`);
await page.waitForSelector(`${dd}[data-dd="values"]`);
await page.waitForFunction(() => /\d/.test(document.querySelector(".fcard .fc-dd [data-ddv] .fc-pkn")?.textContent ?? ""), null, { timeout: 8000 }).catch(() => {});
const vcount = await page.locator(`${dd} [data-ddv] .fc-pkn`).first().innerText();
ok(/\d/.test(vcount), `${tag}: each value counted (${vcount.trim()})`);
await page.click(`${dd} [data-ddv="gap"]`);
await settle(700);
ok(await badge() === "2", `${tag}: a value ticked is the board's at once (badge ${await badge()})`);
ok(await page.locator(named).count() === 0, `${tag}: two rules: the line says no name`);
await page.click(`${dd} [data-ddv="build"]`);
await settle(700);
ok(await page.locator(`${dd} [data-ddv][aria-checked="true"]`).count() === 2 && await badge() === "2", `${tag}: a second value is the same rule (any of)`);
await shot("12-dd-values");
await page.click(`${dd} [data-ddclose]:not(.fc-x)`);
await settle(400);
const listed = `${line} .fc-rule[data-rule*="build"]`;
const opText = () => page.locator(`${listed} [data-fc="neg"]`).innerText().catch((e) => `none: ${e.message.split("\n")[0]}`);
ok(/is any of/.test(await opText()), `${tag}: drawn is any of (${await opText()}; ${await page.locator(listed).getAttribute("data-rule").catch(() => "-")})`);
await negate(listed, 1);
ok(/none of/.test(await opText()), `${tag}: is not makes it none of (${await opText()})`);
await page.locator(`${listed} [data-fc="bin"]`).click();
await settle(700);
ok(await badge() === "1" && await page.locator(named).count() === 1, `${tag}: the bin takes it off the board; the name is back`);
// More: Save as filter…, Reset.
await page.click(`${line} .fc-mb`);
ok(await page.locator(`${line} [data-fc="save-line"]`).count() === 1 && await page.locator(`${line} [data-fc="reset"]`).count() === 1, `${tag}: More has Save as filter… and Reset`);
await page.click(`${line} [data-fc="reset"]`);
await settle(900);
ok(await page.locator(named).count() === 0, `${tag}: Reset takes the board back to its default (${await page.locator(".fline .fline-chips").innerText()})`);
await page.keyboard.press("Escape");
await settle(400);
await go(`?g=${encodeURIComponent("not kind doc")}&f=`);
// + add a filter: the builder, on a new rule's fields.
await page.click(".fline [data-first]");
await settle(700);
ok(await page.locator(line).count() === 1 && await page.locator(`${line} .fc-rule.ghost`).count() === 1 && await page.locator(`${dd}[data-dd="field"]`).count() === 1, `${tag}: + add a filter opens the builder on a new rule`);
await page.click(`${dd} [data-dds="Needs you"]`);
await settle(900);
ok(/Needs you/.test(await page.locator(named).innerText().catch(() => "")), `${tag}: a saved filter chosen is the line's name`);
await page.click(`${line} .fc-mb`);
await page.click(`${line} [data-fc="save-line"]`);
await page.fill(`${line} [data-line-name] input`, `Board-${tag}`);
await page.keyboard.press("Enter");
await settle(1500);
ok(await page.locator(`${line} .fc-head .fc-tok.sv`).innerText().then((t) => t.includes(`Board-${tag}`)).catch(() => false), `${tag}: Save as filter… names the board's filter`);
await page.keyboard.press("Escape");
await settle(400);
// The f key opens and closes it.
await page.keyboard.press("f");
await settle(700);
ok(await page.locator(line).count() === 1, `${tag}: f opens the builder`);
await page.keyboard.press("f");
await settle(500);
ok(!(await isOpen()), `${tag}: f again closes it`);
// FILTER-55 (the owner's answer `now`): ⌂ everywhere's filter is the
// builder's too — its chip, F and (a phone) the bar's Filter open it, editable.
const home = `${card}[data-fcard-open="line:global"]`;
const homeChip = '.fline .fchip-g[data-fcard="line:global"]';
ok(await page.locator(homeChip).count() >= 1 && !/not kind doc/.test(await page.locator(homeChip).first().innerText()), `${tag}: ⌂'s chip says no rules (${(await page.locator(homeChip).first().innerText().catch(() => "")).trim()})`);
await page.locator(homeChip).first().click();
await settle(900);
ok(await page.locator(home).count() === 1 && (await page.getAttribute(card, "data-ro")) !== "true", `${tag}: ⌂'s chip opens the builder on it, editable`);
ok(/The filter everywhere/.test(await page.locator(`${home} .fc-head`).innerText().catch(() => "")), `${tag}: its head says whose`);
ok(await page.locator(`${home} [data-fc="add-rule"]`).count() >= 1, `${tag}: rules can be added to it`);
await fits("⌂'s builder");
await shot("07-home");
await page.keyboard.press("Escape");
await settle(400);
await page.keyboard.press("Shift+F");
await settle(700);
ok(await page.locator(home).count() === 1, `${tag}: F opens the builder on ⌂`);
await page.keyboard.press("Escape");
await settle(400);
if (phone) {
  await page.click('#pbar [data-place="filter"]');
  await settle(800);
  ok(await page.locator(home).count() === 1, `${tag}: the bar's Filter opens the builder on ⌂`);
  await page.click(`${home} .fc-head .fc-x`);
  await settle(400);
}

// A surface's own rules: This page in the field dropdown, a row of its own.
const local = async (route, level, field, value, words) => {
  await go(route);
  const b = `.fline [data-fbtn="${level}"]`;
  try { await page.waitForSelector(b, { timeout: 15000 }); } catch { ok(false, `${tag}: ${words}: its Filter button draws`); return; }
  await page.click(b);
  await settle(900);
  const l = `${card}[data-fcard-open="line:${level}"]`;
  ok(await page.locator(l).count() === 1, `${tag}: ${words}: the Filter button opens the builder`);
  // (an empty filter opens on a new rule's fields already)
  if (!(await page.locator(`${dd}[data-dd="field"]`).count())) await page.click(`${l} [data-fc="add-rule"]`);
  await page.waitForSelector(`${dd}[data-dd="field"]`);
  ok(await page.locator(`${dd} [data-ddl="${field}"]`).count() === 1, `${tag}: ${words}: the field dropdown offers This page`);
  await page.click(`${dd} [data-ddl="${field}"]`);
  await page.waitForSelector(`${dd}[data-dd="values"]`);
  await page.click(`${dd} [data-ddv="${value}"]`);
  await settle(700);
  const row = `${l} .fc-local .fc-rule.local[data-local="${field}"]`;
  ok(await page.locator(row).count() === 1, `${tag}: ${words}: the value ticked is a row of its own`);
  ok(await page.locator(".fline .fline-chips .fchip-b, .fline .fchips-g").count() === 0 && /\d/.test(await page.locator(`${b} .fbadge`).innerText().catch(() => "")), `${tag}: ${words}: the line spells it out not, the badge counts it`);
  if (await page.locator(`${dd} [data-ddclose]:not(.fc-x)`).count()) await page.click(`${dd} [data-ddclose]:not(.fc-x)`);
  else await page.keyboard.press("Escape");
  await settle(400);
  await shot(`13-local-${level}`);
  await page.click(`${row} [data-fc="neg"]`);
  await page.waitForSelector(`${dd}[data-dd="op"]`);
  await page.click(`${dd} [data-ddop="1"]`);
  await settle(600);
  ok(/not/.test(await page.locator(`${row} [data-fc="neg"]`).innerText().catch(() => "")), `${tag}: ${words}: its operator is a dropdown (is not)`);
  await page.click(`${row} [data-fc="bin"]`);
  await settle(600);
  ok(await page.locator(row).count() === 0, `${tag}: ${words}: the bin takes it out`);
  await page.keyboard.press("Escape");
  await settle(400);
};
await local("waves/PLEXUS", "waves", "ready", "workable", "the wave board");
await local("inputs", "inputs", "state", "answered", "the inputs");

ok(errs.length === 0, `${tag}: no page errors (${errs.join(" | ")})`);
await browser.close();
if (fails.length) { console.log(`${fails.length} failed`); process.exit(1); }
