// DEPLOY-2: the smoke check's browser half — the app loads, reaches
// `live · WS`, draws the nav, and throws nothing. Usage:
//   node deploy/smoke-browser.mjs http://127.0.0.1:4451/
// IDENTITY-23: when the environment requires login, the app sends the
// browser to /login; with MENGER_SMOKE_USER and MENGER_SMOKE_PASSWORD set,
// the check signs in there first, as a person would. IDENTITY-30: where
// people sign in on a hosted IdP's page (WorkOS), automation signs in on
// menger's own page, `/login?page`, with plexus-idp's password form; the
// check never depends on the hosted IdP.
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
  const ours = new URL(url).host;
  if (new URL(page.url()).host !== ours || new URL(page.url()).pathname === '/login') {
    const [user, password] = [process.env.MENGER_SMOKE_USER, process.env.MENGER_SMOKE_PASSWORD];
    if (!user || !password) throw new Error('the app requires login, and MENGER_SMOKE_USER / MENGER_SMOKE_PASSWORD are not set');
    if (new URL(page.url()).host !== ours || !(await page.locator('#password').count())) {
      await page.goto(new URL('/login?page&next=%2F', url).toString(), { waitUntil: 'domcontentloaded', timeout: 15000 });
    }
    await page.fill('#username', user);
    await page.fill('#password', password);
    await Promise.all([page.waitForURL((u) => u.host === ours && u.pathname !== '/login', { timeout: 15000 }), page.click('button.primary[type=submit]:not(#hosted)')]);
  }
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
