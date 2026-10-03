// DEPLOY-2: the smoke check's browser half — the app loads, reaches
// `live · WS`, draws the nav, and throws nothing. Usage:
//   node deploy/smoke-browser.mjs http://127.0.0.1:4451/
// Playwright comes from perf/node_modules beside this file's checkout.
import { createRequire } from 'node:module';
const require = createRequire(new URL('../perf/', import.meta.url));
const { chromium } = require('playwright');

const url = process.argv[2];
if (!url) {
  console.error('usage: smoke-browser.mjs <app url>');
  process.exit(2);
}
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
let ok = true;
try {
  await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 15000 });
  await page.getByText('live · WS').first().waitFor({ timeout: 20000 });
  await page.locator('#nav [data-nav-id], #nav [data-node-key]').first().waitFor({ timeout: 20000 });
} catch (e) {
  ok = false;
  console.error(`browser: ${e.message.split('\n')[0]}`);
}
if (errors.length) {
  ok = false;
  console.error(`browser: page errors: ${errors.join(' | ')}`);
}
await browser.close();
console.log(ok ? 'browser: live · WS, nav drawn, no page errors' : 'browser: FAILED');
process.exit(ok ? 0 : 1);
