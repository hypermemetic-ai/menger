// DEPLOY-2: the smoke check's browser half — the app loads, reaches
// `live · WS`, draws the nav, and throws nothing. Usage:
//   node deploy/smoke-browser.mjs http://127.0.0.1:4451/
// IDENTITY-23: when the environment requires login, the app sends the
// browser to /login; with MENGER_SMOKE_USER and MENGER_SMOKE_PASSWORD set,
// the check signs in there first, as a person would. IDENTITY-30: a page
// with no password form (the IdP takes passwords: WorkOS) is signed in
// through the IdP's own page instead — the button MENGER_SMOKE_BUTTON
// (default Email), then MENGER_SMOKE_EMAIL and MENGER_SMOKE_EMAIL_PASSWORD
// on the hosted page's email and password fields.
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
  if (new URL(page.url()).pathname === '/login' && (await page.locator('#password').count())) {
    const [user, password] = [process.env.MENGER_SMOKE_USER, process.env.MENGER_SMOKE_PASSWORD];
    if (!user || !password) throw new Error('the app requires login, and MENGER_SMOKE_USER / MENGER_SMOKE_PASSWORD are not set');
    await page.fill('#username', user);
    await page.fill('#password', password);
    await Promise.all([page.waitForURL((u) => u.pathname !== '/login', { timeout: 15000 }), page.click('button.primary')]);
  } else if (new URL(page.url()).pathname === '/login') {
    const [email, password] = [process.env.MENGER_SMOKE_EMAIL, process.env.MENGER_SMOKE_EMAIL_PASSWORD];
    if (!email || !password) throw new Error('the login page takes no password, and MENGER_SMOKE_EMAIL / MENGER_SMOKE_EMAIL_PASSWORD are not set');
    await page.click(`text=Continue with ${process.env.MENGER_SMOKE_BUTTON || 'Email'}`);
    await page.waitForURL((u) => u.host !== ours, { timeout: 20000 });
    await page.locator('input[type=email]').fill(email);
    await page.locator('button[type=submit]').first().click();
    const pw = page.locator('input[type=password]');
    await pw.waitFor({ state: 'visible', timeout: 20000 });
    await page.waitForTimeout(1000);
    await pw.click();
    await pw.pressSequentially(password, { delay: 10 });
    await Promise.all([
      page.waitForURL((u) => u.host === ours && !u.pathname.startsWith('/login'), { timeout: 30000 }),
      page.getByRole('button', { name: /^(sign in|log in|continue)$/i }).first().click(),
    ]);
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
