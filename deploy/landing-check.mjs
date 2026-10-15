#!/usr/bin/env node
// DEPLOY-17: the landing page's render gate. DIR is landing/ exported at a
// commit (deploy/pipeline landing makes it; never the working tree). The two
// pages are served from it on a loopback port and loaded in Chromium at 390
// and 1440 px with motion reduced; any console error, page error, failed
// request, answer of 400 or more, horizontal scroll or missing <title> fails
// the gate, each named. Playwright comes from perf/node_modules, as
// scripts/shot.mjs's does (PLAYWRIGHT_FROM points elsewhere).
//
//   node deploy/landing-check.mjs DIR
import { createRequire } from 'node:module';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const dir = process.argv[2] ? path.resolve(process.argv[2]) : null;
if (!dir || !fs.existsSync(path.join(dir, 'index.html'))) {
  console.error('usage: landing-check.mjs DIR   (DIR holds index.html, style.css, main.js, guide/ …)');
  process.exit(2);
}
const from = process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM.replace(/\/$/, '')}/` : new URL('../perf/', import.meta.url);
const { chromium } = createRequire(from)('playwright');

const TYPES = {
  '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.svg': 'image/svg+xml', '.webp': 'image/webp', '.avif': 'image/avif', '.png': 'image/png', '.jpg': 'image/jpeg',
  '.mp4': 'video/mp4', '.webm': 'video/webm', '.woff2': 'font/woff2', '.json': 'application/json', '.txt': 'text/plain',
};
const server = http.createServer((req, res) => {
  let p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (p.endsWith('/')) p += 'index.html';
  const f = path.join(dir, p);
  if (!f.startsWith(dir + path.sep) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream' });
  fs.createReadStream(f).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const problems = [];
const browser = await chromium.launch();
for (const route of ['/', '/guide/']) {
  for (const width of [390, 1440]) {
    const tag = `${route} at ${width}`;
    const page = await browser.newPage({ viewport: { width, height: 900 }, reducedMotion: 'reduce' });
    page.on('console', (m) => { if (m.type() === 'error') problems.push(`${tag}: console error: ${m.text()}`); });
    page.on('pageerror', (e) => problems.push(`${tag}: page error: ${e.message}`));
    page.on('requestfailed', (r) => problems.push(`${tag}: request failed: ${r.url()} (${r.failure()?.errorText ?? '?'})`));
    page.on('response', (r) => { if (r.status() >= 400) problems.push(`${tag}: ${r.status()} for ${r.url().replace(/^http:\/\/127\.0\.0\.1:\d+/, '')}`); });
    try {
      await page.goto(`http://127.0.0.1:${port}${route}`, { waitUntil: 'load', timeout: 30000 });
      // scroll through, so lazy pictures load and every reveal runs
      await page.evaluate(async () => {
        for (let y = 0; y < document.body.scrollHeight; y += 700) { window.scrollTo(0, y); await new Promise((r) => setTimeout(r, 40)); }
        window.scrollTo(0, 0);
      });
      await new Promise((r) => setTimeout(r, 800));
      const wide = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
      if (wide) problems.push(`${tag}: the page scrolls horizontally`);
      if (!(await page.title())) problems.push(`${tag}: no <title>`);
    } catch (e) {
      problems.push(`${tag}: ${e.message.split('\n')[0]}`);
    }
    await page.close();
  }
}
await browser.close();
server.close();
if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}
console.log('landing: / and /guide/ render clean at 390 and 1440');
