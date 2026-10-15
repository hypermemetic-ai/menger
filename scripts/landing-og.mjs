// DESIGN-52 — the link preview of menger.sh: landing/media/og.jpg, 1200×630,
// the foot of the page itself (the field and the giant wordmark) with the
// promise above it. Serves landing/ on a loopback port, opens the landing
// page, keeps only the foot, and takes it. Rerun when the foot or the
// promise changes, then commit og.jpg:
//   PLAYWRIGHT_FROM=perf node scripts/landing-og.mjs [OUT]
import { createRequire } from 'node:module';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const dir = path.resolve(here, '../landing');
const out = path.resolve(process.argv[2] || path.join(dir, 'media/og.jpg'));
const { chromium } = createRequire(path.resolve(process.env.PLAYWRIGHT_FROM || path.join(here, '../perf')) + '/')('playwright');
const TYPES = { '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.js': 'text/javascript', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };
const server = http.createServer((req, res) => {
  let p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (p.endsWith('/')) p += 'index.html';
  const f = path.join(dir, p);
  if (!f.startsWith(dir + path.sep) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream' });
  fs.createReadStream(f).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1, reducedMotion: 'reduce', colorScheme: 'dark' });
await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: 'networkidle' });
await page.evaluate(() => document.fonts.ready);
await page.evaluate(() => {
  const foot = document.querySelector('.foot');
  for (const el of [...document.body.children]) if (el !== foot) el.remove();
  const wrap = foot.querySelector('.wrap');
  const lead = document.createElement('div');
  lead.className = 'og-lead';
  lead.innerHTML = '<p class="og-kicker">For software teams of people and AI agents</p><p class="og-promise">Agents show you what they’d build, and ask.</p>';
  wrap.prepend(lead);
  const row = foot.querySelector('.foot-row');
  row.innerHTML = '<span class="foot-brand">menger.sh</span><span class="og-right">open source · MIT licence</span>';
});
await page.addStyleTag({ content: `
  html, body { margin: 0; height: 630px; overflow: hidden; background: var(--night); }
  .foot { height: 630px; border: 0; display: flex; }
  .foot > .wrap { display: flex; flex-direction: column; width: 100%; max-width: none; padding: 0 72px; box-sizing: border-box; }
  .og-lead { padding-top: 60px; }
  .og-kicker { margin: 0 0 18px; font-family: var(--mono); font-size: 15px; letter-spacing: .16em; text-transform: uppercase; color: var(--night-dim); }
  .og-promise { margin: 0; font-size: 50px; font-weight: 600; letter-spacing: -.025em; line-height: 1.08; color: var(--night-ink); max-width: 16em; }
  .giant { font-size: 236px; margin: auto 0 .14em; }
  .foot-row { font-size: 14px; padding: 22px 0 26px; }
  .og-right { margin-left: auto; }
` });
await page.waitForTimeout(300);
await page.screenshot({ path: out, type: 'jpeg', quality: 92 });
await browser.close();
server.close();
console.log(out);
