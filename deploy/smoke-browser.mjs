// DEPLOY-2: the smoke check's browser half — the app loads, reaches
// `live · WS`, draws the nav, and throws nothing. Usage:
//   node deploy/smoke-browser.mjs http://127.0.0.1:4451/
// IDENTITY-23: when the environment requires login, the app sends the
// browser to /login; with MENGER_SMOKE_USER and MENGER_SMOKE_PASSWORD set,
// the check signs in there first, as a person would. IDENTITY-30: where
// people sign in with an email field handed to an IdP (WorkOS), automation
// signs in at `/login?password`, plexus-idp's password form; the check never
// depends on the IdP. ACCESS-28: the app's page goes out under its
// Content-Security-Policy (scripts its own, by hash — no 'unsafe-inline'),
// and nothing on the page or in a frame is refused by a policy.
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
// ACCESS-28: a policy's refusal is logged on the console, the frames' too.
const refused = [];
page.on('console', (m) => { if (/Content Security Policy/i.test(m.text())) refused.push(m.text().slice(0, 200)); });
let doc = null;
page.on('response', (r) => { if (r.request().isNavigationRequest() && r.frame() === page.mainFrame()) doc = r; });
let ok = true;
try {
  await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 15000 });
  const ours = new URL(url).host;
  if (new URL(page.url()).host !== ours || new URL(page.url()).pathname === '/login') {
    const [user, password] = [process.env.MENGER_SMOKE_USER, process.env.MENGER_SMOKE_PASSWORD];
    if (!user || !password) throw new Error('the app requires login, and MENGER_SMOKE_USER / MENGER_SMOKE_PASSWORD are not set');
    if (new URL(page.url()).host !== ours || !(await page.locator('#password').count())) {
      await page.goto(new URL('/login?password&next=%2F', url).toString(), { waitUntil: 'domcontentloaded', timeout: 15000 });
    }
    await page.fill('#username', user);
    await page.fill('#password', password);
    await Promise.all([page.waitForURL((u) => u.host === ours && u.pathname !== '/login', { timeout: 15000 }), page.click('form[action="/login"] button.primary[type=submit]')]);
  }
  await page.getByText('live · WS').first().waitFor({ timeout: 20000 });
  await page.locator('#nav [data-nav-id], #nav [data-node-key]').first().waitFor({ timeout: 20000 });
  const csp = (doc && (await doc.headerValue('content-security-policy'))) || '';
  const scripts = csp.split(';').map((d) => d.trim()).find((d) => d.startsWith('script-src ')) || '';
  if (!scripts.includes("'sha256-") || scripts.includes("'unsafe-inline'") || !csp.includes("frame-ancestors 'none'")) {
    throw new Error(`the app's page is not under its policy: ${csp || 'no Content-Security-Policy'}`);
  }
} catch (e) {
  ok = false;
  console.error(`browser: ${e.message.split('\n')[0]}`);
}
if (errors.length) {
  ok = false;
  console.error(`browser: page errors: ${errors.join(' | ')}`);
}
if (refused.length) {
  ok = false;
  console.error(`browser: refused by a policy: ${[...new Set(refused)].join(' | ')}`);
}
await browser.close();
console.log(ok ? 'browser: live · WS, nav drawn, under its policy, nothing refused, no page errors' : 'browser: FAILED');
process.exit(ok ? 0 : 1);
