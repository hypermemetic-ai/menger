// CHAT-21: a chat whose messages carry components, checked in a browser on
// a throwaway server — never prod. For each browser (Chromium, WebKit),
// width (390, 1440) and theme (light, dark), signed in by an agent's key:
//
//   - the chat opens at its newest message; every framed message near the
//     view is a frame stamped with its home (data-chat, data-message), its
//     title "chat message", no data-ticket; a plain message is no frame;
//   - lazy: scrolled through to the top and back, the frames alive at any
//     moment are never more than the framed messages within a screen of the
//     view (the turns' own positions, read at that moment), and never all;
//   - the decision in the newest message is answered by a click in its
//     frame — Clear (when answered), then Keep and Submit — and the frame
//     shows it answered (from the reply and input_changed) — the caller
//     reads the server's record after (chats inputs list);
//   - *source* shows the markdown and hides the frame; ⤢ widens the turn;
//   - no page error, in the app or a frame.
//
// A shot of the view after the answer goes to --out, one per run
// (<browser>.<theme>.<width>.png).
//
//   node scripts/chat-frames-check.mjs --origin http://127.0.0.1:4461 \
//     --key /tmp/ada.key --chat 1 --message 300 --input undated --out /tmp/shots
//
// The chat is seeded by the caller (a message `--message` asking `--input`,
// a choice with an option `keep`, and framed messages enough to scroll).
// Playwright comes from perf/node_modules. Exit 0 all held · 1 a check
// failed (each named) · 2 usage.

import { createRequire } from 'node:module';
import { parseArgs } from 'node:util';
import { mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const { values: opt } = parseArgs({
  options: {
    origin: { type: 'string' },
    key: { type: 'string' },
    chat: { type: 'string' },
    message: { type: 'string' },
    input: { type: 'string', default: 'undated' },
    agent: { type: 'string', default: 'check' },
    out: { type: 'string' },
    browsers: { type: 'string', default: 'chromium,webkit' },
    widths: { type: 'string', default: '390,1440' },
    themes: { type: 'string', default: 'light,dark' },
  },
});
if (!opt.origin || !opt.key || !opt.chat || !opt.message) {
  console.error('usage: chat-frames-check.mjs --origin URL --key FILE --chat N --message M [--input ID] [--out DIR]');
  process.exit(2);
}
const origin = new URL(opt.origin).origin;
const u = new URL(origin);
if (/^(4450|4451|4452|4453|4455|4456|4457|4458)$/.test(u.port) || u.protocol === 'https:') {
  console.error(`${origin}: a throwaway server only — never test or prod`);
  process.exit(2);
}
const hub = `ws://${u.hostname}:${+u.port - 1}`;
const require = createRequire(new URL('../perf/package.json', import.meta.url));
const pw = require('playwright');
const login = await import(new URL('./throwaway-login.mjs', import.meta.url).href);
const { token } = await login.mintSession(hub, resolve(opt.key), opt.agent);
if (opt.out) mkdirSync(opt.out, { recursive: true });

const failures = [];
let passed = 0;
function check(run, what, ok, detail = '') {
  if (ok) passed++;
  else failures.push(`${run}: ${what}${detail ? ` — ${detail}` : ''}`);
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${run}: ${what}${detail ? ` (${detail})` : ''}`);
}

/** The frames alive now, and the framed turns within a screen of the view. */
async function census(page) {
  return page.evaluate(() => {
    const list = document.querySelector('.pmsgs');
    const scrolls = list && getComputedStyle(list).overflowY !== 'visible' && list.scrollHeight > list.clientHeight;
    const top = scrolls ? list.getBoundingClientRect().top : 0;
    const vh = scrolls ? list.clientHeight : innerHeight;
    const near = [...document.querySelectorAll('[data-message-frame]')].filter((h) => {
      const r = h.getBoundingClientRect();
      return r.bottom > top - vh * 1.05 && r.top < top + vh * 2.05;
    }).length;
    const alive = document.querySelectorAll('iframe.mframe-doc[data-message]').length;
    const held = document.querySelectorAll('[data-message-frame]').length;
    return { alive, near, held, vh };
  });
}

async function scrollBy(page, dy) {
  await page.evaluate((dy) => {
    const list = document.querySelector('.pmsgs');
    const scrolls = list && getComputedStyle(list).overflowY !== 'visible' && list.scrollHeight > list.clientHeight;
    if (scrolls) list.scrollTop += dy; else window.scrollBy(0, dy);
  }, dy);
}

async function atTop(page) {
  return page.evaluate(() => {
    const list = document.querySelector('.pmsgs');
    const scrolls = list && getComputedStyle(list).overflowY !== 'visible' && list.scrollHeight > list.clientHeight;
    return (scrolls ? list.scrollTop : scrollY) <= 2;
  });
}

async function run(browserName, width, theme) {
  const name = `${browserName}.${theme}.${width}`;
  let browser;
  try {
    browser = await pw[browserName].launch();
  } catch (e) {
    check(name, `${browserName} launches`, false, e.message.split('\n').find((l) => /missing|not found|Executable/i.test(l))?.trim() ?? e.message.split('\n')[0]);
    return;
  }
  const context = await browser.newContext({ viewport: { width, height: width < 600 ? 844 : 900 }, colorScheme: theme, deviceScaleFactor: 1 });
  await context.addCookies(login.sessionCookies(origin, token));
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(`page: ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error' && !/favicon/.test(m.text())) errors.push(`console: ${m.text()}`); });
  try {
    await page.goto(`${origin}/#/chats/${opt.chat}`);
    // DESIGN-39: the boot's sheet removes itself once the app is up.
    await page.waitForFunction(() => !document.getElementById('boot'), null, { timeout: 30000 });
    const mine = page.locator(`iframe.mframe-doc[data-message="${opt.message}"]`);
    await mine.waitFor({ state: 'attached', timeout: 30000 });
    await page.waitForFunction((m) => document.querySelector(`iframe.mframe-doc[data-message="${m}"]`)?.getAttribute('data-state') !== 'pending', opt.message, { timeout: 30000 });
    const stamp = await mine.evaluate((f) => ({ chat: f.dataset.chat, ticket: f.getAttribute('data-ticket'), title: f.title, sandbox: f.getAttribute('sandbox'), srcdoc: f.hasAttribute('srcdoc') }));
    check(name, 'the newest framed message is a frame stamped with its home', stamp.chat === opt.chat && stamp.ticket === null && stamp.title === 'chat message' && !stamp.srcdoc, JSON.stringify(stamp));
    // Pinned to the newest: the frames drawn above it do not push it away.
    await page.waitForTimeout(1500);
    const pinned = await page.evaluate((m) => {
      const h = document.querySelector(`[data-message-frame="${m}"]`);
      const list = document.querySelector('.pmsgs');
      const scrolls = list && getComputedStyle(list).overflowY !== 'visible' && list.scrollHeight > list.clientHeight;
      const bottom = scrolls ? list.getBoundingClientRect().bottom : innerHeight;
      const r = h.getBoundingClientRect();
      return { frameBottom: Math.round(r.bottom), viewBottom: Math.round(bottom) };
    }, opt.message);
    check(name, 'opens at the newest message, its frame drawn in view', pinned.frameBottom <= pinned.viewBottom + 2 && pinned.frameBottom > 0, JSON.stringify(pinned));
    const plain = await page.evaluate(() => [...document.querySelectorAll('.turn')].filter((t) => !t.querySelector('[data-message-frame]')).every((t) => !t.querySelector('iframe')));
    check(name, 'a plain message is no frame', plain);

    // The answer, by a click in the frame.
    const frame = page.frameLocator(`iframe.mframe-doc[data-message="${opt.message}"]`);
    // The decision's state, as its pill says it (visible text only: the
    // house components keep hidden copies of a word).
    const state = (word) => frame.locator(`text=/^${word}$/i >> visible=true`).first();
    await mine.scrollIntoViewIfNeeded();
    if (await state('answered').isVisible().catch(() => false)) {
      await frame.getByRole('button', { name: /^clear$/i }).click();
      await state('open').waitFor({ timeout: 10000 });
    }
    await frame.getByRole('radio').first().click();
    await frame.getByRole('button', { name: /^submit$/i }).click();
    const shown = await state('answered').waitFor({ timeout: 15000 }).then(() => true, () => false);
    check(name, 'Submit in the frame answers the message input, and the frame shows it answered', shown);
    await page.waitForTimeout(400);
    if (opt.out) await page.screenshot({ path: join(opt.out, `${name}.png`) });

    // *source* and ⤢.
    const holder = page.locator(`[data-message-frame="${opt.message}"]`);
    await holder.locator('[data-act="frame-source"]').click();
    const src = await holder.locator('pre.tsrc').isVisible();
    const hidden = await holder.locator('.mframe').evaluate((e) => getComputedStyle(e).display === 'none');
    check(name, 'source shows the markdown and hides the frame', src && hidden);
    await holder.locator('[data-act="frame-source"]').click();
    await holder.locator('[data-act="frame-wide"]').click();
    check(name, '⤢ widens it', await holder.evaluate((e) => e.classList.contains('twide')));
    await holder.locator('[data-act="frame-wide"]').click();

    // Lazy, through the whole chat and back.
    let worst = { alive: 0, near: 0 };
    let over = null;
    let all = 0;
    for (let i = 0; i < 400 && !(await atTop(page)); i++) {
      const vh = (await census(page)).vh;
      await scrollBy(page, -vh * 0.9);
      await page.waitForTimeout(150);
      let c = await census(page);
      // The observer answers a frame or two after the scroll: an excess
      // is a failure only if it is still there once it has had its turn.
      if (c.alive > c.near) {
        await page.waitForTimeout(400);
        c = await census(page);
      }
      all = Math.max(all, c.alive);
      if (c.alive > worst.alive) worst = c;
      if (c.alive > c.near && !over) over = c;
    }
    check(name, 'scrolled to the top, the frames alive never pass those within a screen of the view', !over, JSON.stringify(over ?? worst));
    check(name, 'never all forty at once', all < 40, `at most ${all}`);
    // Down again: each jump to the end pages the newer messages in.
    // Mounted is not in view (the window mounts two screens out): until
    // the list is at its end twice running with the newest there.
    let back = false;
    for (let i = 0, still = 0; i < 80 && still < 2; i++) {
      const end = await page.evaluate(() => {
        const list = document.querySelector('.pmsgs');
        const scrolls = list && getComputedStyle(list).overflowY !== 'visible' && list.scrollHeight > list.clientHeight;
        const el = scrolls ? list : document.scrollingElement;
        const was = el.scrollTop >= el.scrollHeight - el.clientHeight - 2;
        el.scrollTop = el.scrollHeight;
        return was;
      });
      await page.waitForTimeout(300);
      back = end && (await mine.count()) > 0;
      still = back ? still + 1 : 0;
    }
    if (back) await page.waitForFunction((m) => document.querySelector(`iframe.mframe-doc[data-message="${m}"]`)?.getAttribute('data-state') !== 'pending', opt.message, { timeout: 15000 }).catch(() => {});
    await state('answered').waitFor({ timeout: 10000 }).catch(() => {});
    const again = back && (await state('answered').isVisible().catch(() => false));
    check(name, 'back at the newest, its frame is made again, still answered', again, again ? '' : JSON.stringify({ back, census: await census(page), state: await mine.getAttribute('data-state').catch(() => 'gone') }));
    check(name, 'no page errors', errors.length === 0, errors.slice(0, 3).join(' | '));
  } catch (e) {
    check(name, 'ran', false, e.message.split('\n')[0]);
  } finally {
    await browser.close();
  }
}

for (const b of opt.browsers.split(',')) {
  for (const w of opt.widths.split(',').map(Number)) {
    for (const t of opt.themes.split(',')) await run(b, w, t);
  }
}
console.log(`\n${passed} passed, ${failures.length} failed`);
for (const f of failures) console.log(`  ${f}`);
process.exit(failures.length ? 1 : 0);
