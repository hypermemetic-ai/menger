// CHAT-61: a message frame loading above the view never moves the chat —
// checked in a browser on a throwaway server, never prod. At 390 × 844, in
// `--browser` (WebKit: no overflow-anchor at all; Chromium: with
// overflow-anchor: none forced on the page, so only the app's own anchoring
// holds it), signed in by an agent's key, twice — a fresh tab (no height
// kept: every frame mounts at a guess) and a reload (the heights kept in
// localStorage):
//
//   - the chat opens at its newest turn; it is scrolled up 0.6 of a screen
//     at a time to the top; after each step the first turn seen (the first
//     that begins in the view) is watched every animation frame for 1.2 s while what the step
//     brought near loads (frames made, sized, settled, stats counting, older
//     turns paged in) — its top never moves more than 1 px;
//   - it opens at the newest turn and stays there (CHAT-21's rule);
//   - something did load above it on the way (the column changed height
//     while watched), so the check is not vacuous;
//   - on the reload, the frames mount at their kept heights: the column
//     changes height less often than on the fresh tab;
//   - no page error.
//
//   node scripts/chat-anchor-check.mjs --origin http://127.0.0.1:4631 --key /tmp/op.key --chat 1 --browser webkit
//
// scripts/chat-anchor-check.sh seeds the chat and runs it. Playwright comes
// from perf/node_modules (else ../menger/perf/node_modules). Exit 0 all held
// · 1 a check failed (each named) · 2 usage.

import { createRequire } from 'node:module';
import { parseArgs } from 'node:util';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';

const { values: opt } = parseArgs({
  options: {
    origin: { type: 'string' },
    key: { type: 'string' },
    chat: { type: 'string' },
    browser: { type: 'string', default: 'webkit' },
    agent: { type: 'string', default: 'builder' },
    width: { type: 'string', default: '390' },
    tolerance: { type: 'string', default: '1' },
  },
});
if (!opt.origin || !opt.key || !opt.chat) {
  console.error('usage: chat-anchor-check.mjs --origin URL --key FILE --chat N [--browser webkit|chromium]');
  process.exit(2);
}
const origin = new URL(opt.origin).origin;
const u = new URL(origin);
if (/^(4450|4451|4452|4453|4455|4456|4457|4458)$/.test(u.port) || u.protocol === 'https:') {
  console.error(`${origin}: a throwaway server only — never test or prod`);
  process.exit(2);
}
const hub = `ws://${u.hostname}:${+u.port - 1}`;
const own = new URL('../perf/node_modules/playwright', import.meta.url);
const pkg = new URL(existsSync(own) ? '../perf/package.json' : '../../menger/perf/package.json', import.meta.url);
const pw = createRequire(pkg)('playwright');
const login = await import(new URL('./throwaway-login.mjs', import.meta.url).href);
const { token } = await login.mintSession(hub, resolve(opt.key), opt.agent);
const tolerance = Number(opt.tolerance);

const failures = [];
let passed = 0;
function check(run, what, ok, detail = '') {
  if (ok) passed++;
  else failures.push(`${run}: ${what}${detail ? ` — ${detail}` : ''}`);
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${run}: ${what}${detail ? ` (${detail})` : ''}`);
}

/** Scroll up `frac` of a screen, then watch the first turn seen. */
function stepAndWatch([frac, ms]) {
  const list = document.querySelector('.pmsgs');
  const scrolls = list && getComputedStyle(list).overflowY !== 'visible' && list.scrollHeight > list.clientHeight;
  const el = scrolls ? list : document.scrollingElement;
  const top = scrolls ? list.getBoundingClientRect().top : 0;
  const vh = scrolls ? list.clientHeight : innerHeight;
  if (el.scrollTop <= 2) return { atTop: true };
  el.scrollTop = Math.max(0, el.scrollTop - vh * frac);
  // The first turn seen: the first that begins inside the view, under the
  // full chat's sticky head where the document scrolls (a change inside
  // the view moves what is below it; only what is above is held).
  const head = scrolls ? 0 : (document.querySelector('#chatfull .shead')?.offsetHeight ?? 0);
  const row = [...document.querySelectorAll('.trow[data-mid]')].find((r) => r.getBoundingClientRect().top >= top + head - 0.5);
  if (!row) return { atTop: false, none: true };
  const id = row.dataset.mid;
  const col = document.querySelector('.tcol');
  const at = () => document.querySelector(`.trow[data-mid="${id}"]`)?.getBoundingClientRect().top;
  const t0 = at();
  let h = col.getBoundingClientRect().height;
  let changes = 0;
  let worst = 0;
  let gone = false;
  // What is painted: read each frame at its animation callback, and again
  // after the frame's resize observers (the app's anchoring among them,
  // created before this one) when they run — the last read is the frame's.
  let frame = null;
  const commit = () => {
    if (frame === undefined) gone = true;
    else if (frame !== null) worst = Math.max(worst, Math.abs(frame - t0));
  };
  const ro = new ResizeObserver(() => {
    frame = at();
  });
  ro.observe(col);
  return new Promise((done) => {
    const start = performance.now();
    const tick = () => {
      commit();
      frame = at();
      const nh = col.getBoundingClientRect().height;
      if (Math.abs(nh - h) > 0.5) changes++;
      h = nh;
      if (performance.now() - start < ms) requestAnimationFrame(tick);
      else {
        ro.disconnect();
        done({ atTop: false, id, worst: Math.round(worst * 10) / 10, changes, gone, scrollTop: Math.round(el.scrollTop) });
      }
    };
    requestAnimationFrame(tick);
  });
}

async function walk(page, name, pass) {
  await page.waitForFunction(() => !document.getElementById('boot'), null, { timeout: 30000 });
  await page.waitForFunction(() => [...document.querySelectorAll('iframe.mframe-doc[data-message]')].some((f) => f.getAttribute('data-state') !== 'pending'), null, { timeout: 30000 });
  await page.waitForTimeout(1500);
  // CHAT-21's rule still holds: it opens at the newest, and the frames
  // drawn above it while it loads leave it there.
  const off = await page.evaluate(() => {
    const list = document.querySelector('.pmsgs');
    const scrolls = getComputedStyle(list).overflowY !== 'visible' && list.scrollHeight > list.clientHeight;
    const el = scrolls ? list : document.scrollingElement;
    return Math.round(el.scrollHeight - el.scrollTop - el.clientHeight);
  });
  check(name, `${pass}: opens at the newest turn and stays there while the frames above load`, off <= 48, `${off} px off the bottom`);
  let worst = { worst: 0 };
  let changes = 0;
  let steps = 0;
  let frames = 0;
  for (let i = 0; i < 120; i++) {
    const r = await page.evaluate(stepAndWatch, [0.6, 1200]);
    if (r.atTop) break;
    if (r.none) continue;
    steps++;
    changes += r.changes;
    if (!r.gone && r.worst > worst.worst) worst = r;
    frames = Math.max(frames, await page.evaluate(() => document.querySelectorAll('iframe.mframe-doc[data-message]').length));
  }
  check(name, `${pass}: scrolled up through the chat (${steps} steps), the turn in view moved ≤ ${tolerance} px while what is above it loaded`, steps > 3 && worst.worst <= tolerance, JSON.stringify(worst));
  check(name, `${pass}: frames loaded above the view on the way`, frames > 0 && (pass === 'reload' || changes > 0), `frames ${frames}, column height changes ${changes}`);
  return changes;
}

async function run() {
  const name = `${opt.browser}.${opt.width}`;
  let browser;
  try {
    browser = await pw[opt.browser].launch();
  } catch (e) {
    check(name, `${opt.browser} launches`, false, e.message.split('\n')[0]);
    return;
  }
  const width = Number(opt.width);
  const context = await browser.newContext({ viewport: { width, height: 844 }, deviceScaleFactor: 1 });
  await context.addCookies(login.sessionCookies(origin, token));
  if (opt.browser !== 'webkit') {
    // Chromium anchors by itself: turned off, as WebKit has none.
    await context.addInitScript(() => {
      addEventListener('DOMContentLoaded', () => {
        const s = document.createElement('style');
        s.textContent = '*, html, body { overflow-anchor: none !important; }';
        document.head.append(s);
      });
    });
  }
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(`page: ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error' && !/favicon/.test(m.text())) errors.push(`console: ${m.text()}`); });
  try {
    await page.goto(`${origin}/#/chats/${opt.chat}`);
    const fresh = await walk(page, name, 'fresh tab');
    const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('menger.cframe-heights') ?? '[]').length);
    check(name, 'the frames\' heights are kept beyond the tab', kept > 0, `${kept} kept`);
    await page.reload();
    const again = await walk(page, name, 'reload');
    check(name, 'on the reload the frames mount at their kept heights (fewer height changes)', again < fresh, `fresh ${fresh}, reload ${again}`);
    if (opt.browser !== 'webkit') {
      const off = await page.evaluate(() => getComputedStyle(document.querySelector('.pmsgs')).overflowAnchor);
      check(name, 'overflow-anchor is off: the app held it', off === 'none', off);
    }
    check(name, 'no page errors', errors.length === 0, errors.slice(0, 3).join(' | '));
  } catch (e) {
    check(name, 'ran', false, e.message.split('\n')[0]);
  } finally {
    await browser.close();
  }
}

await run();
console.log(`\n${passed} passed, ${failures.length} failed`);
for (const f of failures) console.log(`  ${f}`);
process.exit(failures.length ? 1 : 0);
