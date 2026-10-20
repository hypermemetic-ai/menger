#!/usr/bin/env node
// CHAT-58: a HEIC attached in a chat's composer, in WebKit (and Chromium
// beside it). Against a throwaway server holding chat CHAT
// (scripts/throwaway-login.mjs makes the session file):
//   - scripts/fixtures/tiny.heic (a real HEIC, 64 × 48, made with libheif):
//     where the browser decodes it, it is kept as a JPEG (the draft's
//     thumbnail's bytes begin FF D8); where it cannot (Chromium, WebKit on
//     Linux), the refusal says "HEIC can't be read in this browser; send it
//     as JPEG or PNG", and nothing is put;
//     (the kept image's type and bytes read back with media.get: a page's
//     CSP keeps it from fetching its own blob: URLs);
//   - the conversion, in any browser: a file said to be image/heic and
//     named .heic whose bytes the browser decodes (a PNG) is kept as a
//     JPEG; with alpha, as a PNG;
//   - no page errors.
//
//   node scripts/heic-check.mjs http://127.0.0.1:PORT SESSION_FILE CHAT [HUB]
//
// HUB is the server's WebSocket (default ws://<host>:<PORT - 1>). WebKit
// runs in the Playwright image (docker, --network host).
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
import zlib from "node:zlib";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

import { call, connect } from "../deploy/menger-call.mjs";

const [origin, tokenFile, chat, hubArg] = process.argv.slice(2);
const token = fs.readFileSync(tokenFile, "utf8").trim();
const u = new URL(origin);
const cookie = { name: `menger_session_${u.port}`, value: token, url: `${origin}/`, httpOnly: true, sameSite: "Lax" };
const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };
const until = async (f, ms = 10000) => { const end = Date.now() + ms; while (Date.now() < end) { const v = await f(); if (v) return v; await new Promise((r) => setTimeout(r, 150)); } return null; };

/** A PNG `w` × `h`, every pixel `rgba`. */
function png(w, h, rgba) {
  const crcT = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (b) => { let c = 0xffffffff; for (const x of b) c = crcT[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (t, d) => { const len = Buffer.alloc(4); len.writeUInt32BE(d.length); const td = Buffer.concat([Buffer.from(t), d]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([len, td, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6;
  const row = Buffer.concat([Buffer.from([0]), Buffer.concat(Array.from({ length: w }, () => Buffer.from(rgba)))]);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

const heic = fs.readFileSync(new URL("./fixtures/tiny.heic", import.meta.url));
const hub = hubArg ?? `ws://${u.hostname}:${+u.port - 1}`;

/** The kept image `hash`: its type and its first four bytes, in hex. */
async function kept(hash) {
  const ws = await connect(hub, { Authorization: `Bearer ${token}` });
  let got = null;
  try {
    await call(ws, 1, "media.get", { hash }, (item) => {
      if (item?.content?.type === "media") got = { mime: item.content.mime, magic: Buffer.from(item.content.data, "base64").subarray(0, 4).toString("hex") };
    });
  } finally {
    ws.close();
  }
  return got;
}

const UNREADABLE = "HEIC can't be read in this browser; send it as JPEG or PNG";

/** Attach `file`; the outcome: { kept, mime, magic } (the kept image's
 *  type and first bytes) or { why } (the refusal on its thumbnail). */
async function attach(page, file) {
  const before = await page.$$eval("#chatfull .cthumb[data-media-hash]", (xs) => xs.length);
  await page.setInputFiles("#chatfull input[type=file]", file);
  return until(async () => {
    const why = await page.$eval("#chatfull .cup-why", (e) => e.textContent).catch(() => null);
    if (why) return { why };
    const hashes = await page.$$eval("#chatfull .cthumb[data-media-hash]", (xs) => xs.map((x) => x.dataset.mediaHash));
    if (hashes.length <= before) return null;
    const got = await kept(hashes[hashes.length - 1]);
    return got ? { kept: true, ...got } : null;
  });
}

async function clear(page) {
  for (const x of await page.$$("#chatfull .cup-remove, #chatfull .cthumb-x")) await x.click().catch(() => {});
  await page.fill("#chatfull textarea", "");
  await until(async () => (await page.$$("#chatfull .cup, #chatfull .cthumb")).length === 0, 3000);
}

for (const engine of ["webkit", "chromium"]) {
  const browser = await pw[engine].launch();
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  await ctx.addCookies([cookie]);
  const page = await ctx.newPage();
  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));
  await page.goto(`${origin}/#/chats/${chat}`);
  await page.waitForSelector("#chatfull textarea", { timeout: 30000 });

  const real = await attach(page, { name: "Screenshot 2026-10-19 at 11.00.25 PM.heic", mimeType: "image/heic", buffer: heic });
  if (real?.kept) ok(real.mime === "image/jpeg" && real.magic.startsWith("ffd8"), `${engine}: the HEIC decoded, kept as a JPEG (${real.mime} ${real.magic})`);
  else ok(real?.why === `Screenshot 2026-10-19 at 11.00.25 PM.heic: ${UNREADABLE}`, `${engine}: the HEIC it cannot read is refused plainly (${real?.why})`);
  console.log(`     ${engine} ${real?.kept ? "decodes" : "does not decode"} HEIC`);
  await clear(page);

  const opaque = await attach(page, { name: "IMG_0001.HEIC", mimeType: "image/heic", buffer: png(40, 30, [200, 80, 40, 255]) });
  ok(opaque?.kept && opaque.mime === "image/jpeg" && opaque.magic.startsWith("ffd8"), `${engine}: a decodable "HEIC" is kept as a JPEG (${JSON.stringify(opaque)})`);
  await clear(page);

  const alpha = await attach(page, { name: "cutout.heic", mimeType: "image/heif", buffer: png(40, 30, [200, 80, 40, 100]) });
  ok(alpha?.kept && alpha.mime === "image/png" && alpha.magic === "89504e47", `${engine}: with alpha, kept as a PNG (${JSON.stringify(alpha)})`);
  await clear(page);

  ok(errs.length === 0, `${engine}: no page errors (${errs.join(" | ")})`);
  await browser.close();
}

if (fails.length) {
  console.log(`\n${fails.length} failed`);
  process.exit(1);
}
console.log("\nall ok");
