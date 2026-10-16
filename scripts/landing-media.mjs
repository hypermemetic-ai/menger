#!/usr/bin/env node
// MENGER-10: captures the landing page's stills and clips from a throwaway
// menger seeded by scripts/landing-media.sh (which runs this, then encodes).
//
//   node scripts/landing-media.mjs --port 4471 --ws-port 4470 --raw DIR [--only NAME,...]
//
// Stills: DIR/stills/<name>.<light|dark>.png at 1440×900, 2× — they look
// only (nothing is saved). Clips: DIR/clips/<name>.mp4, near-lossless, from
// Chromium's screencast — these write (a status moved, a decision answered),
// so they run after the stills, on a freshly seeded store.
//
// A pointer and key caps are drawn over the page in the clips so a viewer
// can follow what is clicked and typed; everything under them is the app.
import { createRequire } from 'node:module';
import { parseArgs } from 'node:util';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const { values: opt } = parseArgs({
  options: {
    port: { type: 'string', default: '4471' },
    'ws-port': { type: 'string', default: '4470' },
    raw: { type: 'string' },
    only: { type: 'string' },
  },
});
if (!opt.raw) throw new Error('--raw DIR is required');
for (const p of [opt.port, opt['ws-port']]) if (+p >= 4450 && +p <= 4458) throw new Error(`port ${p} is prod's or test's`);
const base = `http://127.0.0.1:${opt.port}/`;
const only = opt.only ? new Set(opt.only.split(',')) : null;
const want = (name) => !only || only.has(name);
// Playwright from perf/node_modules (cd perf && npm ci), or PLAYWRIGHT_FROM
// (a directory whose node_modules has it — a worktree borrows the checkout's).
const from = process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM.replace(/\/$/, '')}/` : new URL('../perf/', import.meta.url);
const { chromium } = createRequire(from)('playwright');

const STILLS = join(opt.raw, 'stills');
const CLIPS = join(opt.raw, 'clips');
mkdirSync(STILLS, { recursive: true });
mkdirSync(CLIPS, { recursive: true });

const synapse = (...args) => execFileSync('synapse', ['-P', opt['ws-port'], 'menger', 'tickets', ...args], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The app as a first-time visitor on this server sees it, minus the header's
// link to menger's own test environment (it points at the page itself), and
// with the sample's tree open in the nav.
function prime() {
  try {
    localStorage.setItem('menger.settings', JSON.stringify({ version: 1, ws_url: null, recent: [], test_url: location.origin }));
    if (!localStorage.getItem('menger.nav.expanded')) {
      localStorage.setItem('menger.nav.expanded', JSON.stringify(['OFFLINE', 'OFFLINE-1', 'SYNC', 'SYNC-1']));
    }
  } catch {}
}

/** Waits for the app's own signals: the socket live, nothing loading, every
 *  framed body settled; then a quiet spell (scripts/shot.mjs's rule, short). */
async function ready(page, quiet = 900) {
  await page.waitForFunction(
    () => {
      const conn = document.getElementById('conn');
      if (!conn || !/^live\b/.test(conn.textContent.trim())) return false;
      const main = document.querySelector('main');
      if (!main) return false;
      const loading = [...main.querySelectorAll('.loading, p.none')].some((el) => /^(loading|placing)\b/i.test(el.textContent.trim()));
      return !loading && document.fonts.status === 'loaded';
    },
    null,
    { timeout: 30000 },
  );
  await sleep(quiet);
}

async function open(browser, { route, theme = 'light', width = 1440, height = 900, scale = 2, navHidden = false }) {
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, colorScheme: theme });
  await context.addInitScript(prime);
  if (navHidden) await context.addInitScript(() => { try { localStorage.setItem('menger.nav.hidden', '1'); } catch {} });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error(`page error (${route}): ${e.message.split('\n')[0]}`));
  await page.goto(base + route);
  await ready(page, 1500);
  return page;
}

// ── stills ────────────────────────────────────────────────────────────────

const stills = [
  { name: 'board', route: '#/' },
  { name: 'ticket', route: '#/ticket/OFFLINE-6' },
  { name: 'waves', route: '#/waves/OFFLINE' },
  {
    name: 'filter',
    route: '#/',
    act: async (p) => {
      await p.keyboard.press('f');
      await sleep(500);
      await p.keyboard.type('rea', { delay: 60 });
      await sleep(400);
    },
  },
  {
    name: 'palette',
    route: '#/',
    act: async (p) => {
      await p.keyboard.press('/');
      await sleep(400);
      await p.keyboard.type('photo', { delay: 60 });
      await sleep(600);
    },
  },
  {
    name: 'decision',
    route: '#/ticket/OFFLINE-7',
    act: async (p) => {
      const f = p.frameLocator('iframe').first();
      // the option picked, not yet submitted (the rationale stays empty: the
      // house components' rationale box takes no keystrokes today — its
      // onkeydown returns false for every key but Ctrl+Enter)
      await f.getByRole('radio').first().click();
      await sleep(600);
    },
  },
  {
    name: 'annotate',
    route: '#/ticket/OFFLINE-10',
    act: async (p) => {
      await p.keyboard.press('a');
      await sleep(500);
      await p.click('#ticket-page h2');
      await sleep(400);
      await p.keyboard.type('Repro on a Pixel 6 too: the photos vanish after a restart.', { delay: 10 });
      await sleep(400);
    },
  },
  // DESIGN-48 — what the agents put inside their tickets, and the tray.
  {
    // the agent's mockup of the conflict screen with its decision, and the
    // tab the queue put at the bottom of the screen (queued for this shot
    // alone, so no other still carries the tray)
    name: 'mockup',
    route: '#/ticket/OFFLINE-14',
    act: async (p) => {
      synapse('queue', '--id', 'OFFLINE-14', '--urgency', 'now', '--input', 'layout', '--reason', 'OFFLINE-8 is idle until the layout is picked');
      await sleep(4200); // the arrival's flash and label are over; the dot stays
      await p.evaluate(() => { document.querySelector('#app .content').scrollTop = 100; });
      await sleep(600);
    },
    after: () => synapse('unqueue', '--id', 'OFFLINE-14'),
  },
  {
    // DESIGN-48: the help page an agent built, filed as a doc with
    // view: mockup — the ticket draws it at 390 and 1440 side by side, the
    // bar's toggles above. The whole app (owner: "show the nav"): a taller
    // screen, the ticket's title at the top, the nav and the header kept.
    name: 'page',
    route: '#/ticket/OFFLINE-16',
    height: 1040,
    act: async (p) => {
      await p.waitForSelector('.framed .device iframe', { timeout: 30000 });
      await sleep(1800); // both frames painted; nothing scrolled: the header, the title, the frames
    },
  },
  {
    // the team's own component, on its help page: the docs, the example
    // live, who defines it and who uses it, the source
    name: 'components',
    route: '#/components/fn-banner',
    act: async (p) => {
      await p.waitForSelector('iframe', { timeout: 30000 });
      await sleep(1800);
    },
  },
  {
    // the components doc itself: both components in every state
    name: 'kit',
    route: '#/ticket/OFFLINE-17',
    height: 1040,
    act: async (p) => {
      await p.waitForSelector('iframe', { timeout: 30000 });
      await sleep(1800);
      await p.evaluate(() => { document.querySelector('#app .content').scrollTop = 480; });
      await sleep(600);
    },
  },
  {
    // the team on the agent's plan: annotation mode on OFFLINE-15, the pin
    // of the two comments left on its title clicked, so they are listed
    // over the map and the decision
    name: 'collab',
    route: '#/ticket/OFFLINE-15',
    height: 1040,
    act: async (p) => {
      await p.waitForSelector('iframe', { timeout: 30000 });
      await sleep(1800);
      await p.evaluate(() => { document.querySelector('#app .content').scrollTop = 110; });
      await sleep(400);
      await p.keyboard.press('a');
      await sleep(1200);
      await p.click('.annot-pin:not(.moved)');
      await sleep(800);
    },
  },
  { name: 'deploys', route: '#/deployments' },
];

/** The ticket page's scroller moved so its framed box starts `pad` px under the top. */
async function scrollToFramed(p, pad) {
  await p.evaluate((pad) => {
    const c = document.querySelector('#app .content');
    const box = document.querySelector('.framed');
    c.scrollTop = box.getBoundingClientRect().top - c.getBoundingClientRect().top + c.scrollTop - pad;
  }, pad);
}

async function takeStills(browser) {
  for (const s of stills) {
    if (!want(s.name)) continue;
    for (const theme of ['light', 'dark']) {
      const page = await open(browser, { route: s.route, theme, height: s.height || 900 });
      if (s.act) await s.act(page);
      await sleep(300);
      const path = join(STILLS, `${s.name}.${theme}.png`);
      await page.screenshot({ path });
      console.error(`still ${path}`);
      if (s.after) await s.after(page);
      await page.context().close();
    }
  }
}

// ── clips ─────────────────────────────────────────────────────────────────

/** Chromium's screencast of one page: JPEG frames with their times. */
async function record(page) {
  const cdp = await page.context().newCDPSession(page);
  const frames = [];
  cdp.on('Page.screencastFrame', (f) => {
    // the time it arrived, on the same clock as t0/t1 (the frame's own
    // timestamp is on the compositor's, which drifts from Date.now)
    frames.push({ data: f.data, t: Date.now() / 1000 });
    cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
  });
  const vp = page.viewportSize();
  await cdp.send('Page.startScreencast', { format: 'jpeg', quality: 95, maxWidth: vp.width * 2, maxHeight: vp.height * 2, everyNthFrame: 1 });
  return {
    async stop() {
      await cdp.send('Page.stopScreencast');
      return frames;
    },
  };
}

/** Frames → a constant-rate, near-lossless mp4 over [t0, t1]. */
function toVideo(frames, t0, t1, out, width) {
  const dir = `${out}.frames`;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  // the frame showing at t0 (the last one before it), then every later one
  const before = frames.filter((f) => f.t <= t0);
  const kept = [...before.slice(-1), ...frames.filter((f) => f.t > t0 && f.t <= t1)];
  if (!kept.length) throw new Error(`no frames for ${out}`);
  let list = '';
  kept.forEach((f, i) => {
    const file = join(dir, `${String(i).padStart(5, '0')}.jpg`);
    writeFileSync(file, Buffer.from(f.data, 'base64'));
    const start = i === 0 ? t0 : f.t;
    const end = i + 1 < kept.length ? kept[i + 1].t : t1;
    list += `file '${file}'\nduration ${Math.max(0.001, end - start).toFixed(4)}\n`;
  });
  list += `file '${join(dir, `${String(kept.length - 1).padStart(5, '0')}.jpg`)}'\n`;
  writeFileSync(`${dir}/list.txt`, list);
  execFileSync('ffmpeg', [
    '-loglevel', 'error', '-y', '-f', 'concat', '-safe', '0', '-i', `${dir}/list.txt`,
    // -t: the concat demuxer holds the last image past its duration
    '-t', (t1 - t0).toFixed(3), '-vf', `fps=30,scale=${width}:-2:flags=lanczos`, '-c:v', 'libx264', '-crf', '8', '-preset', 'fast', '-pix_fmt', 'yuv420p', out,
  ]);
  rmSync(dir, { recursive: true, force: true });
}

// The pointer and key caps — drawn over the app so the clip can be followed.
function overlay() {
  if (document.getElementById('lm-pointer')) return;
  const style = document.createElement('style');
  style.textContent = `
    #lm-pointer { position: fixed; left: 0; top: 0; z-index: 2147483647; pointer-events: none;
      width: 22px; height: 22px; transform: translate(-40px, 300px);
      transition: transform .65s cubic-bezier(.3,.8,.3,1); }
    #lm-pointer svg { width: 22px; height: 22px; filter: drop-shadow(0 1px 2px rgba(0,0,0,.45)); }
    #lm-pointer i { position: absolute; left: -9px; top: -9px; width: 18px; height: 18px; border-radius: 50%;
      background: rgba(26,95,180,.35); transform: scale(0); opacity: 0; }
    #lm-pointer.click i { animation: lm-click .45s ease-out; }
    @keyframes lm-click { from { transform: scale(.3); opacity: 1; } to { transform: scale(1.8); opacity: 0; } }
    #lm-keys { position: fixed; right: 18px; bottom: 18px; z-index: 2147483647; pointer-events: none;
      display: flex; gap: 6px; }
    #lm-keys kbd { font: 600 15px/1 ui-monospace, monospace; color: #fff; background: rgba(20,20,26,.82);
      border-radius: 7px; padding: 8px 11px; box-shadow: 0 2px 8px rgba(0,0,0,.3); }`;
  document.documentElement.append(style);
  const p = document.createElement('div');
  p.id = 'lm-pointer';
  p.innerHTML = '<i></i><svg viewBox="0 0 22 22"><path d="M3 2l14 9.5-6.2 1.1 3.6 6.6-2.6 1.4-3.6-6.6L3 18z" fill="#fff" stroke="#111" stroke-width="1.3" stroke-linejoin="round"/></svg>';
  document.documentElement.append(p);
  const k = document.createElement('div');
  k.id = 'lm-keys';
  document.documentElement.append(k);
}
async function pointTo(page, locator, { click = true } = {}) {
  const box = await locator.boundingBox();
  const x = box.x + Math.min(box.width / 2, 40), y = box.y + box.height / 2;
  await page.evaluate(([x, y]) => { document.getElementById('lm-pointer').style.transform = `translate(${x}px, ${y}px)`; }, [x, y]);
  await sleep(750);
  if (click) {
    await page.evaluate(() => {
      const p = document.getElementById('lm-pointer');
      p.classList.remove('click'); void p.offsetWidth; p.classList.add('click');
    });
  }
}
/** Every device frame of the page's mockup scrolled to `at` (0 top, 1 the
 *  bottom) of its document, together, eased over `ms`. */
async function scrollFrames(page, at, ms) {
  const frames = [];
  for (const h of await page.$$('.framed .device iframe')) { const f = await h.contentFrame(); if (f) frames.push(f); }
  await Promise.all(frames.map((f) => f.evaluate(([at, ms]) => new Promise((done) => {
    const el = document.scrollingElement, from = el.scrollTop, to = (el.scrollHeight - el.clientHeight) * at, t0 = performance.now();
    const ease = (t) => (t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2);
    const step = (now) => { const t = Math.min(1, (now - t0) / ms); el.scrollTop = from + (to - from) * ease(t); t < 1 ? requestAnimationFrame(step) : done(); };
    requestAnimationFrame(step);
  }), [at, ms])));
}

/** The drawn pointer and the real mouse both to the middle of `locator`. */
async function hover(page, locator) {
  const box = await locator.boundingBox();
  const x = box.x + box.width / 2, y = box.y + box.height / 2;
  await page.evaluate(([x, y]) => { document.getElementById('lm-pointer').style.transform = `translate(${x}px, ${y}px)`; }, [x, y]);
  await page.mouse.move(x, y);
  await sleep(700);
}
async function keycap(page, label, ms = 900) {
  await page.evaluate(([label, ms]) => {
    const k = document.getElementById('lm-keys');
    const kb = document.createElement('kbd');
    kb.textContent = label;
    k.replaceChildren(kb);
    setTimeout(() => kb.remove(), ms);
  }, [label, ms]);
}

/** One clip of one page: open, overlay, record `act`, hold, write. */
async function clip(browser, name, { route, width = 1200, height = 750, out = 1200, hold = 1400, navHidden = false, setup, act }) {
  if (!want(name)) return;
  const page = await open(browser, { route, width, height, scale: 1, navHidden });
  if (setup) await setup(page);
  await page.evaluate(overlay);
  await sleep(300);
  const rec = await record(page);
  const t0 = Date.now() / 1000;
  await sleep(500);
  await act(page);
  await sleep(hold);
  const t1 = Date.now() / 1000;
  const frames = await rec.stop();
  const path = join(CLIPS, `${name}.mp4`);
  toVideo(frames, t0, t1, path, out);
  console.error(`clip ${path} (${(t1 - t0).toFixed(1)} s, ${frames.length} frames)`);
  await page.context().close();
}

async function takeClips(browser) {
  // 1 · find: the board narrows to what can start now
  await clip(browser, 'tour-1-find', {
    route: '#/',
    act: async (p) => {
      await pointTo(p, p.locator('[data-ftext=board]'));
      await p.click('[data-ftext=board]');
      await sleep(400);
      await p.keyboard.type('ready', { delay: 110 });
      await sleep(500);
      await keycap(p, 'Enter');
      await p.keyboard.press('Enter');
      await sleep(900);
      await pointTo(p, p.locator('main table tbody tr').first(), { click: false });
    },
    hold: 1200,
  });

  // 2 · the lifecycle: a worker claims OFFLINE-10, links its commit, hands it
  //     to review; the merge completes it. Driven over the wire, as an agent would.
  await clip(browser, 'tour-2-lifecycle', {
    route: '#/ticket/OFFLINE-10',
    width: 1000,
    height: 625,
    out: 1000,
    act: async () => {
      await sleep(300);
      synapse('set_status', '--id', 'OFFLINE-10', '--status', 'Coding');
      await sleep(1300);
      synapse('commits', 'link', '--id', 'OFFLINE-10', '--commits', 'a7c31f0');
      await sleep(1000);
      synapse('set_status', '--id', 'OFFLINE-10', '--status', 'In Review');
      await sleep(1300);
      synapse('set_status', '--id', 'OFFLINE-10', '--status', 'Complete');
    },
    hold: 1600,
  });

  // 3 · live: two windows; the board on the left, the wave board on the right.
  //     Completing OFFLINE-6 on the left moves the cards on the right.
  if (want('tour-3-live')) {
    const W = 800, H = 750;
    const left = await open(browser, { route: '#/', width: W, height: H, scale: 1, navHidden: true });
    const right = await open(browser, { route: '#/waves/OFFLINE', width: W, height: H, scale: 1, navHidden: true });
    await left.evaluate(overlay);
    await sleep(300);
    const recL = await record(left), recR = await record(right);
    const t0 = Date.now() / 1000;
    await sleep(900);
    const sel = left.locator('main tr', { has: left.locator('a', { hasText: /^OFFLINE-6$/ }) }).locator('select.status');
    await pointTo(left, sel);
    await sel.selectOption('Complete');
    await sleep(2600);
    const t1 = Date.now() / 1000;
    const [fl, fr] = [await recL.stop(), await recR.stop()];
    toVideo(fl, t0, t1, join(CLIPS, 'live-left.mp4'), W);
    toVideo(fr, t0, t1, join(CLIPS, 'live-right.mp4'), W);
    execFileSync('ffmpeg', [
      '-loglevel', 'error', '-y', '-i', join(CLIPS, 'live-left.mp4'), '-i', join(CLIPS, 'live-right.mp4'),
      '-filter_complex', '[0]pad=iw+24:ih+24:12:12:color=0x3a3d46[a];[1]pad=iw+12:ih+24:0:12:color=0x3a3d46[b];[a][b]hstack,scale=1280:-2:flags=lanczos',
      '-c:v', 'libx264', '-crf', '8', '-preset', 'fast', '-pix_fmt', 'yuv420p', join(CLIPS, 'tour-3-live.mp4'),
    ]);
    rmSync(join(CLIPS, 'live-left.mp4'));
    rmSync(join(CLIPS, 'live-right.mp4'));
    console.error(`clip ${join(CLIPS, 'tour-3-live.mp4')} (${(t1 - t0).toFixed(1)} s)`);
    await left.context().close();
    await right.context().close();
  }

  // 4 · decide: the owner answers the decision inside the ticket
  await clip(browser, 'tour-4-decide', {
    route: '#/ticket/OFFLINE-7',
    height: 800,
    setup: async (p) => {
      await p.evaluate(() => { document.querySelector('#app .content').scrollTop = 95; });
      await sleep(600);
    },
    act: async (p) => {
      const f = p.frameLocator('iframe').first();
      const opt = f.getByRole('radio').first();
      await pointTo(p, opt);
      await opt.click();
      await sleep(700);
      const submit = f.getByRole('button', { name: /submit/i }).first();
      await pointTo(p, submit);
      await submit.click();
    },
    hold: 1800,
  });

  // 7 · the hero (DESIGN-48; owner: "the first should show both screens
  //     scrolling"): the whole app, nav and header, the help page in its
  //     ticket at 390 and 1440 — both frames scroll down together, rest, and
  //     come back up. No pointer: nothing is clicked.
  await clip(browser, 'hero-scroll', {
    route: '#/ticket/OFFLINE-16',
    width: 1440,
    height: 980,
    out: 1440,
    setup: async (p) => {
      await p.waitForSelector('.framed .device iframe', { timeout: 30000 });
      await sleep(1800);
      await p.evaluate(() => { document.getElementById('lm-pointer')?.remove(); });
    },
    act: async (p) => {
      await p.evaluate(() => { const el = document.getElementById('lm-pointer'); if (el) el.style.display = 'none'; });
      await sleep(600);
      await scrollFrames(p, 0.62, 3200);
      await sleep(1200);
      await scrollFrames(p, 1.0, 1800);
      await sleep(1000);
      await scrollFrames(p, 0, 2400);
    },
    hold: 900,
  });

  // 6 · the queue: scripts/ask-recorded.mjs since DESIGN-55 — the agent's
  //     terminal beside the app, answering OFFLINE-15's `shape` (which a
  //     clip here would answer first)

  // 5 · jump: the palette, from the keyboard
  await clip(browser, 'tour-5-jump', {
    route: '#/',
    act: async (p) => {
      await keycap(p, '/');
      await p.keyboard.press('/');
      await sleep(500);
      await p.keyboard.type('conflict', { delay: 110 });
      await sleep(700);
      await keycap(p, 'Enter');
      await p.keyboard.press('Enter');
      await sleep(600);
    },
    hold: 1800,
  });
}

const browser = await chromium.launch();
try {
  await takeStills(browser);
  await takeClips(browser);
} finally {
  await browser.close();
}
