#!/usr/bin/env node
// APP-104: mark up an image, in a browser, at 390 (iPhone 13, Chromium).
// Against a throwaway (scripts/markup-check.sh) whose chat CHAT shows the
// image HASH in a turn: a tap opens it full size with *mark up* on its bar;
// mark up opens the composer's pad over the image, at its aspect; a stroke
// drawn and *attach* tapped, the pad is put away and the composer holds a
// new image (markup, not HASH); no page errors.
//
//   node scripts/markup-check.mjs ORIGIN SESSION_FILE CHAT HASH THEME [SHOTS]
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const [origin, tokenFile, chat, hash, scheme, shots] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const tag = `390-${scheme}`;

const browser = await pw.chromium.launch();
const ctx = await browser.newContext({ ...pw.devices["iPhone 13"], colorScheme: scheme });
await ctx.addCookies([cookie]);
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push(String(e)));
const done = async () => {
  ok(errs.length === 0, `${tag}: no page errors ${errs.join(" | ")}`);
  await browser.close();
  if (fails.length) { console.log(`${fails.length} failed`); process.exit(1); }
  console.log("all ok");
  process.exit(0);
};
await page.goto(`${origin}/app/#/chats/${chat}`);
const img = `#chatfull img.cimg[data-media="${hash}"][data-state="drawn"]`;
try {
  await page.waitForSelector(img, { timeout: 30000 });
} catch {
  if (shots) await page.screenshot({ path: `${shots}/${tag}-no-image.png` });
  ok(false, `${tag}: the turn's image drew`);
  await done();
}
await page.waitForTimeout(800);
await page.tap(img);
const mark = await page.waitForSelector(".lightbox .lb-bar button.lb-markup", { timeout: 5000 }).catch(() => null);
ok(!!mark, `${tag}: a tap opens it full size with mark up on its bar`);
if (!mark) await done();
const look = await mark.evaluate((b) => ({ words: b.textContent, first: b === b.parentElement.firstElementChild, bg: getComputedStyle(b).backgroundColor }));
ok(look.words === "mark up" && look.first, `${tag}: mark up is the bar's first button (${look.words})`);
ok(look.bg !== "rgba(0, 0, 0, 0)" && look.bg !== "transparent", `${tag}: and filled, on a phone (${look.bg})`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-lightbox.png` });
await mark.tap();
const surf = await page.waitForSelector("#chatfull .dpad .dsurf.over canvas.dink", { timeout: 5000 }).catch(() => null);
ok(!!surf, `${tag}: mark up opens the composer's pad over the image`);
if (!surf) await done();
ok(!(await page.$(".lightbox")), `${tag}: the full-size view closed`);
await page.waitForTimeout(400);
const box = await surf.evaluate((c) => { const r = c.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height, vh: innerHeight }; });
ok(Math.abs(box.w - box.h) < 3, `${tag}: at the image's aspect (${Math.round(box.w)}×${Math.round(box.h)}, the image is square)`);
ok(box.h <= box.vh * 0.5, `${tag}: at most about half the screen tall (${Math.round(box.h)} of ${box.vh})`);
// The pad paints the image under the strokes: its middle is not blank.
const painted = await surf.evaluate((c) => { const d = c.getContext("2d").getImageData(Math.floor(c.width / 2), Math.floor(c.height / 2), 1, 1).data; return d[3] > 0; });
ok(painted, `${tag}: the image is painted on the pad`);
// A stroke, as a finger draws it.
await surf.evaluate((c, b) => {
  const at = (type, x, y) => c.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "touch", isPrimary: true, clientX: b.x + x, clientY: b.y + y }));
  at("pointerdown", b.w * 0.2, b.h * 0.2);
  for (let i = 1; i <= 10; i++) at("pointermove", b.w * (0.2 + i * 0.05), b.h * (0.2 + i * 0.05));
  at("pointerup", b.w * 0.7, b.h * 0.7);
}, box);
await page.waitForTimeout(200);
const attach = await page.$("#chatfull .dpad button[data-act=attach]");
ok(!!attach && !(await attach.isDisabled()), `${tag}: a stroke drawn, attach is on`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-pad.png` });
if (attach) await attach.tap();
let value = "";
for (let i = 0; i < 40; i++) {
  value = await page.$eval("#chatfull .ccompose textarea", (t) => t.value).catch(() => "");
  if (/media:[0-9a-f]{64}/.test(value)) break;
  await page.waitForTimeout(250);
}
const m = value.match(/!\[([^\]]*)\]\(media:([0-9a-f]{64})\)/);
ok(!!m && m[2] !== hash, `${tag}: the composer holds the marked-up image, a new one (${m ? `${m[1]} ${m[2].slice(0, 8)}…` : value})`);
ok(!(await page.$("#chatfull .dpad")), `${tag}: the pad is put away`);
ok(!!(await page.$("#chatfull .ccompose .cthumb")), `${tag}: its thumbnail is in the box`);
if (shots) await page.screenshot({ path: `${shots}/${tag}-attached.png` });
await done();
