#!/usr/bin/env node
// DESIGN-55: the ask, recorded on two screens — on the left a terminal where
// an agent (a Claude Code session, drawn) queues its question and listens;
// on the right the app, where the tab arrives and is opened and answered;
// then the terminal's listener hitting the answer. A replay of a real
// exchange (DESIGN-54's: "queue the question in a ticket", the decision
// queued at now, answered in the tray, await_reply returning) on the sample
// project: every command shown in the terminal is run, against the
// throwaway menger `scripts/landing-media.sh seed` leaves running, and what
// it prints is what the terminal shows. Only the session around them — the
// prompt, the agent's sentences, the spinner — is drawn.
//
//   node scripts/ask-recorded.mjs --port 4471 --ws-port 4470 --session FILE --raw DIR
//
// --session FILE: a session token on that server (ACCESS-52; minted by
// scripts/landing-media.sh): the synapse calls carry it as --token-file
// (the terminal shows them without it), the app's page as its cookie.
//
// Output: DIR/clips/ask-recorded.mp4 — both screens side by side, 2080×860,
// near-lossless; scripts/landing-media.sh runs it after landing-media.mjs and
// encodes it with the other clips (this one up to 1600 px wide, so the
// terminal reads). It writes to the server (a queue, an answer, a close):
// run it on a freshly seeded one.
import { createRequire } from 'node:module';
import { readToken, signInContext } from './throwaway-login.mjs';
import { parseArgs } from 'node:util';
import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const { values: opt } = parseArgs({
  options: {
    port: { type: 'string', default: '4471' },
    'ws-port': { type: 'string', default: '4470' },
    raw: { type: 'string' },
    session: { type: 'string' },
  },
});
if (!opt.raw) throw new Error('--raw DIR is required');
if (!opt.session) throw new Error('--session FILE is required (a session token on the throwaway server: scripts/throwaway-login.mjs)');
const TOKEN = readToken(opt.session);
const signed = (args) => ['--token-file', opt.session, ...args];
for (const p of [opt.port, opt['ws-port']]) if (+p >= 4450 && +p <= 4458) throw new Error(`port ${p} is prod's or test's`);
const base = `http://127.0.0.1:${opt.port}/`;
const WS = opt['ws-port'];
const from = process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM.replace(/\/$/, '')}/` : new URL('../perf/', import.meta.url);
const { chromium } = createRequire(from)('playwright');
const CLIPS = join(opt.raw, 'clips');
mkdirSync(CLIPS, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The exchange. The ticket, its input and the option the owner picks are
// the sample's (scripts/landing-media.sh: OFFLINE-15, `shape`, `one`).
const T = 'OFFLINE-15', INPUT = 'shape';
const REASON = 'the other help pages wait on this: one page or three?';
const QUEUE = ['-P', WS, 'menger', 'tickets', 'queue', '--id', T, '--urgency', 'now', '--input', INPUT, '--reason', REASON];
const AWAIT = ['-P', WS, '-j', 'menger', 'tickets', 'await_reply', '--id', T, '--input', INPUT, '--timeout-minutes', '5'];
const CLOSE = ['-P', WS, 'menger', 'tickets', 'inputs', 'close', '--ticket', T, '--input', INPUT];
const shown = (args) => 'synapse ' + args.map((a) => (/[\s?:;]/.test(a) ? `"${a}"` : a)).join(' ');

// ── the terminal: Konsole, a Claude Code session in it ────────────────────

const MONO = readFileSync(new URL('../landing/fonts/geist-mono-latin.woff2', import.meta.url)).toString('base64');
const TERMINAL = `<!doctype html><meta charset="utf-8"><style>
@font-face { font-family: GM; src: url(data:font/woff2;base64,${MONO}) format('woff2'); font-weight: 100 900; }
:root { --bg: #1b1b1f; --ink: #e8e6e3; --dim: #8f8c88; --faint: #5d5a57; --claude: #d97757; --ok: #4eba65; --line: #3a3a40; }
* { box-sizing: border-box; }
html, body { margin: 0; height: 100%; background: #0f0f12; }
body { padding: 14px; font: 14.5px/1.5 GM, 'DejaVu Sans Mono', 'Noto Sans Mono', 'Noto Sans Symbols 2', 'Noto Sans Symbols', monospace; color: var(--ink); }
.win { height: 100%; display: flex; flex-direction: column; border-radius: 10px; overflow: hidden; background: var(--bg);
  box-shadow: 0 0 0 1px #2c2c33, 0 18px 50px rgba(0,0,0,.55); }
.bar { height: 34px; flex: none; display: flex; align-items: center; gap: 10px; padding: 0 12px; background: #2a2a30; color: #c9c6c1;
  font: 500 13px/1 system-ui, 'Noto Sans', sans-serif; border-bottom: 1px solid #222; }
.bar .ico { width: 16px; height: 16px; border-radius: 3px; background: #31363b; box-shadow: inset 0 0 0 1px #4d5257; position: relative; }
.bar .ico::after { content: '>_'; position: absolute; left: 2px; top: 1px; font: 700 9px/1 GM, monospace; color: #9fd0ff; }
.bar .t { flex: 1; text-align: center; }
.bar .b { width: 14px; height: 14px; border-radius: 50%; background: #45454c; }
.tabs { height: 30px; flex: none; display: flex; align-items: flex-end; padding: 0 8px; background: #232328; border-bottom: 1px solid var(--line); }
.tabs span { font: 12.5px/1 system-ui, sans-serif; color: #d6d3ce; padding: 8px 14px; background: var(--bg); border: 1px solid var(--line); border-bottom: 0; border-radius: 6px 6px 0 0; }
.scr { flex: 1; display: flex; flex-direction: column; padding: 14px 18px 12px; overflow: hidden; }
.log { flex: 1; overflow: hidden; display: flex; flex-direction: column; justify-content: flex-end; }
.welcome { border: 1px solid var(--claude); border-radius: 8px; padding: 10px 14px; margin-bottom: 18px; flex: none; }
.welcome b { color: var(--claude); font-weight: 500; }
.welcome p { margin: 0; } .welcome .d { color: var(--dim); margin-top: 8px; }
.row { display: flex; gap: 10px; margin: 0 0 12px; white-space: pre-wrap; word-break: break-word; }
.row .g { flex: none; width: 1ch; }
.row.user { color: var(--dim); } .row.user .g { color: var(--dim); }
.row.say .g { color: var(--ink); }
.row.tool .g { color: var(--faint); } .row.tool.done .g { color: var(--ok); } .row.tool.run .g { animation: blink 1s steps(2) infinite; }
.row.tool b { font-weight: 700; } .row.tool .args { color: var(--ink); }
.res { margin: -8px 0 12px 2ch; color: var(--dim); white-space: pre-wrap; display: flex; gap: 10px; }
.res .g { flex: none; }
.res em { font-style: normal; color: var(--ink); }
strong { font-weight: 700; color: #fff; }
@keyframes blink { 50% { opacity: .25; } }
.spin { height: 22px; color: var(--claude); margin: 2px 0 8px; flex: none; }
.spin .k { color: var(--dim); }
.box { flex: none; border: 1px solid #55555c; border-radius: 8px; padding: 8px 12px; display: flex; gap: 10px; min-height: 40px; }
.box .p { color: var(--ink); } .box .c { display: inline-block; width: .6em; background: var(--ink); animation: blink 1s steps(2) infinite; }
.hint { flex: none; color: var(--faint); font-size: 12.5px; padding: 6px 2px 0; display: flex; justify-content: space-between; }
</style>
<div class="win">
  <div class="bar"><i class="ico"></i><span class="t">claude — ~/dev/fieldnote — Konsole</span><i class="b"></i><i class="b"></i><i class="b"></i></div>
  <div class="tabs"><span>claude</span></div>
  <div class="scr">
    <div class="log" id="log">
      <div class="welcome"><p><b>✻</b> Welcome to <strong>Claude Code</strong>!</p><p class="d">  cwd: ~/dev/fieldnote</p></div>
    </div>
    <div class="spin" id="spin"></div>
    <div class="box"><span class="p">&gt;</span><span><span id="typed"></span><span class="c"></span></span></div>
    <div class="hint"><span>? for shortcuts</span><span>menger · synapse -P ${WS}</span></div>
  </div>
</div>
<script>
const log = document.getElementById('log'), spinEl = document.getElementById('spin'), typed = document.getElementById('typed');
const esc = (s) => s.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]);
const md = (s) => esc(s).replace(/\\*\\*(.+?)\\*\\*/g, '<strong>$1</strong>');
function add(html, cls) { const d = document.createElement('div'); d.className = cls; d.innerHTML = html; log.append(d); return d; }
let spinT = null;
window.term = {
  type(ch) { typed.textContent += ch; },
  submit() { add('<span class="g">&gt;</span><span>' + esc(typed.textContent) + '</span>', 'row user'); typed.textContent = ''; },
  say(text) { add('<span class="g">⏺</span><span>' + md(text) + '</span>', 'row say'); },
  tool(name, args) { const d = add('<span class="g">⏺</span><span><b>' + esc(name) + '</b>(<span class="args">' + esc(args) + '</span>)</span>', 'row tool run'); d.id = 'tool-' + log.children.length; return d.id; },
  done(id) { const d = document.getElementById(id); d.classList.remove('run'); d.classList.add('done'); },
  result(text, first = true) { add('<span class="g">' + (first ? '⎿' : ' ') + '</span><span>' + md(text) + '</span>', 'res'); },
  spin(text) {
    clearInterval(spinT); const glyphs = '·✢✳✶✻✽✻✶✳✢'; let i = 0; const t0 = Date.now();
    if (!text) { spinEl.textContent = ''; return; }
    const draw = () => { spinEl.innerHTML = glyphs[i++ % glyphs.length] + ' ' + esc(text) + ' <span class="k">(' + Math.round((Date.now() - t0) / 1000) + 's · esc to interrupt)</span>'; };
    draw(); spinT = setInterval(draw, 120);
  },
};
</script>`;

// ── the app ───────────────────────────────────────────────────────────────

function prime() {
  try {
    localStorage.setItem('menger.settings', JSON.stringify({ version: 1, ws_url: null, recent: [], test_url: location.origin }));
    if (!localStorage.getItem('menger.nav.expanded')) localStorage.setItem('menger.nav.expanded', JSON.stringify(['OFFLINE', 'OFFLINE-1']));
  } catch {}
}
async function ready(page, quiet = 900) {
  await page.waitForFunction(() => {
    const conn = document.getElementById('conn');
    if (!conn || !/^live\b/.test(conn.textContent.trim())) return false;
    const main = document.querySelector('main');
    if (!main) return false;
    const loading = [...main.querySelectorAll('.loading, p.none')].some((el) => /^(loading|placing)\b/i.test(el.textContent.trim()));
    return !loading && document.fonts.status === 'loaded';
  }, null, { timeout: 30000 });
  await sleep(quiet);
}
// the pointer drawn over the app (scripts/landing-media.mjs's)
function overlay() {
  const style = document.createElement('style');
  style.textContent = `
    #lm-pointer { position: fixed; left: 0; top: 0; z-index: 2147483647; pointer-events: none; width: 22px; height: 22px;
      transform: translate(1320px, 420px); transition: transform .65s cubic-bezier(.3,.8,.3,1); }
    #lm-pointer svg { width: 22px; height: 22px; filter: drop-shadow(0 1px 2px rgba(0,0,0,.45)); }
    #lm-pointer i { position: absolute; left: -9px; top: -9px; width: 18px; height: 18px; border-radius: 50%;
      background: rgba(26,95,180,.35); transform: scale(0); opacity: 0; }
    #lm-pointer.click i { animation: lm-click .45s ease-out; }
    @keyframes lm-click { from { transform: scale(.3); opacity: 1; } to { transform: scale(1.8); opacity: 0; } }`;
  document.documentElement.append(style);
  const p = document.createElement('div');
  p.id = 'lm-pointer';
  p.innerHTML = '<i></i><svg viewBox="0 0 22 22"><path d="M3 2l14 9.5-6.2 1.1 3.6 6.6-2.6 1.4-3.6-6.6L3 18z" fill="#fff" stroke="#111" stroke-width="1.3" stroke-linejoin="round"/></svg>';
  document.documentElement.append(p);
}
async function pointAndClick(page, locator) {
  const box = await locator.boundingBox();
  const x = box.x + Math.min(box.width / 2, 40), y = box.y + box.height / 2;
  await page.evaluate(([x, y]) => { document.getElementById('lm-pointer').style.transform = `translate(${x}px, ${y}px)`; }, [x, y]);
  await sleep(750);
  await page.evaluate(() => { const p = document.getElementById('lm-pointer'); p.classList.remove('click'); void p.offsetWidth; p.classList.add('click'); });
  await locator.click();
}

// ── recording (scripts/landing-media.mjs's screencast, two pages on one clock)

async function record(page) {
  const cdp = await page.context().newCDPSession(page);
  const frames = [];
  cdp.on('Page.screencastFrame', (f) => {
    frames.push({ data: f.data, t: Date.now() / 1000 });
    cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
  });
  const vp = page.viewportSize();
  await cdp.send('Page.startScreencast', { format: 'jpeg', quality: 95, maxWidth: vp.width, maxHeight: vp.height, everyNthFrame: 1 });
  return { async stop() { await cdp.send('Page.stopScreencast'); return frames; } };
}
function toVideo(frames, t0, t1, out) {
  const dir = `${out}.frames`;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const kept = [...frames.filter((f) => f.t <= t0).slice(-1), ...frames.filter((f) => f.t > t0 && f.t <= t1)];
  if (!kept.length) throw new Error(`no frames for ${out}`);
  let list = '';
  kept.forEach((f, i) => {
    const file = join(dir, `${String(i).padStart(5, '0')}.jpg`);
    writeFileSync(file, Buffer.from(f.data, 'base64'));
    const start = i === 0 ? t0 : f.t, end = i + 1 < kept.length ? kept[i + 1].t : t1;
    list += `file '${file}'\nduration ${Math.max(0.001, end - start).toFixed(4)}\n`;
  });
  list += `file '${join(dir, `${String(kept.length - 1).padStart(5, '0')}.jpg`)}'\n`;
  writeFileSync(`${dir}/list.txt`, list);
  execFileSync('ffmpeg', ['-loglevel', 'error', '-y', '-f', 'concat', '-safe', '0', '-i', `${dir}/list.txt`,
    '-t', (t1 - t0).toFixed(3), '-vf', 'fps=30', '-c:v', 'libx264', '-crf', '8', '-preset', 'fast', '-pix_fmt', 'yuv420p', out]);
  rmSync(dir, { recursive: true, force: true });
}

// ── the run ───────────────────────────────────────────────────────────────

const H = 860, TW = 800, AW = 1280;
const browser = await chromium.launch();
try {
  const termCtx = await browser.newContext({ viewport: { width: TW, height: H }, deviceScaleFactor: 1 });
  const term = await termCtx.newPage();
  await term.setContent(TERMINAL);
  await term.evaluate(() => document.fonts.ready);

  const appCtx = await browser.newContext({ viewport: { width: AW, height: H }, deviceScaleFactor: 1, colorScheme: 'light' });
  await signInContext(appCtx, base, TOKEN);
  await appCtx.addInitScript(prime);
  const app = await appCtx.newPage();
  app.on('pageerror', (e) => console.error(`page error: ${e.message.split('\n')[0]}`));
  await app.goto(base + '#/');
  await ready(app, 1500);
  await app.evaluate(overlay);

  const T_ = (fn, ...a) => term.evaluate(([fn, a]) => window.term[fn](...a), [fn, a]);
  const recT = await record(term), recA = await record(app);
  await sleep(400);
  const t0 = Date.now() / 1000;
  await sleep(900);

  // the owner's words, typed
  for (const ch of 'queue the question in a ticket') { await T_('type', ch); await sleep(45); }
  await sleep(450);
  await T_('submit');
  await T_('spin', 'Thinking…');
  await sleep(1300);
  await T_('spin', '');
  await T_('say', `I'll queue ${T}'s decision **${INPUT}** at now, so it reaches you wherever the app is open.`);
  await sleep(900);

  // queue — run for real; the app hears it
  let id = await T_('tool', 'Bash', shown(QUEUE));
  await T_('spin', 'Queueing…');
  await sleep(700);
  const queued = execFileSync('synapse', signed(QUEUE), { encoding: 'utf8' }).trim().split('\n').filter((l) => /^type:/.test(l)).pop() || 'type: queued';
  await T_('spin', '');
  await T_('done', id);
  await T_('result', queued);
  await sleep(900);
  await T_('say', 'Queued. Listening for your answer.');
  await sleep(500);

  // await_reply — run for real, its lines as they come
  id = await T_('tool', 'Bash', shown(AWAIT));
  const replied = new Promise((resolve, reject) => {
    const child = spawn('synapse', signed(AWAIT));
    let buf = '';
    child.stdout.on('data', async (d) => {
      buf += d;
      let nl;
      while ((nl = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, nl); buf = buf.slice(nl + 1);
        let ev; try { ev = JSON.parse(line); } catch { continue; }
        const c = ev.content || {};
        if (c.type === 'waiting') {
          await T_('result', `waiting · ${c.ticket} · queued ${c.queued?.urgency ?? ''} · input ${c.queued?.input ?? INPUT}`);
          await T_('spin', 'Waiting for your answer…');
        } else if (c.type === 'replied') {
          const r = c.reply || {};
          await T_('spin', '');
          await T_('done', id);
          await T_('result', `replied · ${r.kind} · ${r.input} = **${r.value?.option ?? r.value?.text ?? '?'}** (${r.source})`, false);
          resolve(r);
        }
      }
    });
    child.on('error', reject);
    child.on('exit', (code) => code && reject(new Error(`await_reply exited ${code}`)));
  });

  // the app: the tab arrives (already, on the queue), is opened, answered
  await sleep(2600);
  const tab = app.locator('#qtray .qtab').first();
  await pointAndClick(app, tab);
  await sleep(2200); // the sheet: the reason and the question
  await pointAndClick(app, app.locator('#qtray .qopt.rec').first());
  await sleep(900);
  await pointAndClick(app, app.locator('#qtray .qbtn.fill').first());

  const reply = await Promise.race([replied, sleep(20000).then(() => { throw new Error('no reply in 20 s'); })]);
  await app.mouse.move(1180, 80); // the real mouse too: off the board, so no row stays lit
  await app.evaluate(() => { document.getElementById('lm-pointer').style.transform = 'translate(1180px, 80px)'; });
  await sleep(1100);
  await T_('say', `You picked **${reply.value?.option === 'one' ? 'One page' : reply.value?.option}**. Closing the input and building the rest of the help as one page.`);
  await sleep(700);
  id = await T_('tool', 'Bash', shown(CLOSE));
  await sleep(500);
  const closed = execFileSync('synapse', signed(CLOSE), { encoding: 'utf8' }).trim().split('\n').filter((l) => /^type:/.test(l)).pop() || 'type: input_changed';
  await T_('done', id);
  await T_('result', closed);
  await sleep(2800);
  const t1 = Date.now() / 1000;
  const [ft, fa] = [await recT.stop(), await recA.stop()];

  const L = join(CLIPS, 'ask-term.mp4'), R = join(CLIPS, 'ask-app.mp4'), out = join(CLIPS, 'ask-recorded.mp4');
  toVideo(ft, t0, t1, L);
  toVideo(fa, t0, t1, R);
  execFileSync('ffmpeg', ['-loglevel', 'error', '-y', '-i', L, '-i', R, '-filter_complex', '[0:v][1:v]hstack=inputs=2,fps=30',
    '-c:v', 'libx264', '-crf', '8', '-preset', 'fast', '-pix_fmt', 'yuv420p', out]);
  rmSync(L); rmSync(R);
  console.error(`clip ${out} (${(t1 - t0).toFixed(1)} s, ${ft.length} + ${fa.length} frames)`);
} finally {
  await browser.close();
}
