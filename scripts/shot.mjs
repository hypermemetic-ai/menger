#!/usr/bin/env node
// DEPLOY-7: one screenshot script — menger as it is, once it has drawn.
//
// It waits on the app's own signals, never on a timer: the socket live
// (`#conn` reads `live …`), the route drawn (no `loading…` left in <main>),
// every framed body settled (the SDK's `menger:size { settled: true }` —
// diagrams drawn, the registry in hand, its calls answered), fonts and
// images loaded in the page and in every frame, and the layout still for a
// quiet spell. A timeout names what it was still waiting for.
//
// It only looks: no clicks, no typing, no writes. Signing in is the one
// thing it submits, and only where the environment sends it to /login.
// Playwright comes from perf/node_modules, as deploy/smoke-browser.mjs's.
import { createRequire } from 'node:module';
import { parseArgs } from 'node:util';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const HELP = `usage: scripts/shot.mjs TARGET... [options]

Screenshots menger once it is ready: the socket live, the route drawn, every
framed body settled, fonts loaded, the layout still. It never clicks, types
or writes; it is safe on prod.

TARGET is one of
  '#/ticket/FILTER-13'      an app route, on --env or --port
  http://host:port/#/board  a URL
  page.html                 a local file (no app checks)

options
  --env NAME          prod (:4456) or test (:4451): the port and the sign-in
                      account come from ~/.config/menger/NAME.env  [prod]
  --port N            the app on 127.0.0.1:N instead of --env
  --theme T           light, dark or both                          [light]
  --width W[,W...]    viewport widths in px, comma-separated       [1440]
  --height H          viewport height in px                        [900]
  --reduced-motion    emulate prefers-reduced-motion: reduce, so the app's
                      Motion setting reads none (its default follows it)
  --screen            only the visible screen (default: the whole page —
                      the window grows to the tallest scroller)
  --selector CSS      only this element (the first match), whole unless
                      --screen
  --wait-for CSS      also wait until this matches — the signal for a page
                      that has none of the app's (a late-drawn local file)
  --out PATH          a directory, or a .png file when there is one shot
                                                  [$TMPDIR/menger-shots]
  --as USER           sign in through the dev sign-in (IDENTITY-35) as USER
                      (a roster user keeps its grants; a new name is made a
                      viewer on *), where the environment offers it
  --page              a URL that is not the app: skip the app checks
  --timeout S         seconds to wait for each page to be ready    [45]
  -h, --help          this text

Signing in: where the app redirects to /login, the script signs in once per
run — as --as USER through /login/dev, else as MENGER_SMOKE_USER with
MENGER_SMOKE_PASSWORD (the environment's file, or the process environment)
through /login. Prod is open; nothing is submitted there.

Files are named ENV.ROUTE.THEME.WIDTH.png, e.g.
  prod.ticket-DESIGN-3.dark.400.png
Exit status: 0 when every shot was written, 1 when one failed, 2 on usage.`;

const usage = (msg) => {
  console.error(`shot: ${msg}\nTry 'scripts/shot.mjs --help'.`);
  process.exit(2);
};

let parsed;
try {
  parsed = parseArgs({
    allowPositionals: true,
    strict: true,
    options: {
      env: { type: 'string' },
      port: { type: 'string' },
      theme: { type: 'string', default: 'light' },
      width: { type: 'string', default: '1440' },
      height: { type: 'string', default: '900' },
      screen: { type: 'boolean', default: false },
      'reduced-motion': { type: 'boolean', default: false },
      selector: { type: 'string' },
      'wait-for': { type: 'string' },
      out: { type: 'string' },
      as: { type: 'string' },
      page: { type: 'boolean', default: false },
      timeout: { type: 'string', default: '45' },
      help: { type: 'boolean', short: 'h', default: false },
    },
  });
} catch (e) {
  // parseArgs names the flag it refused ("Unknown option '--wdith'").
  usage(e.message.replace(/\. To specify a positional.*$/s, ''));
}
const opt = parsed.values;
if (opt.help) {
  console.log(HELP);
  process.exit(0);
}
if (!parsed.positionals.length) usage('no target: give a route (#/board), a URL or an .html file');
if (opt.env && opt.port) usage('--env and --port both given: pick one');

const int = (name, v, min, max) => {
  if (!/^\d+$/.test(v) || +v < min || +v > max) usage(`--${name} ${v}: a whole number from ${min} to ${max}`);
  return +v;
};
const themes = { light: ['light'], dark: ['dark'], both: ['light', 'dark'] }[opt.theme];
if (!themes) usage(`--theme ${opt.theme}: light, dark or both`);
const widths = opt.width.split(',').map((w) => int('width', w.trim(), 200, 4000));
const height = int('height', opt.height, 200, 4000);
const timeoutMs = int('timeout', opt.timeout, 1, 600) * 1000;
// Chromium will not paint a surface taller than this in one screenshot.
const MAX_HEIGHT = 16000;

// ── Where: an environment's file, a port, a URL, a file ──────────────────

const ENVS = { prod: 4456, test: 4451 };
const envName = opt.env ?? (opt.port ? null : 'prod');
if (envName && !(envName in ENVS)) usage(`--env ${envName}: prod or test`);

/** KEY=VALUE lines of an env file (systemd's EnvironmentFile shape). */
function readEnvFile(path) {
  const vars = {};
  if (!existsSync(path)) return vars;
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    const m = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*$/);
    if (!m) continue;
    let v = m[2];
    if (/^(["']).*\1$/.test(v)) v = v.slice(1, -1);
    vars[m[1]] = v;
  }
  return vars;
}
const confDir = join(homedir(), '.config', 'menger');
// The env file that describes this server: the named one, or the one whose
// shell port is --port (so a port still finds its sign-in account).
let envVars = {};
let label = envName;
let port = envName ? ENVS[envName] : int('port', opt.port, 1, 65535);
if (envName) {
  envVars = readEnvFile(join(confDir, `${envName}.env`));
  if (envVars.MENGER_SHELL_PORT) port = +envVars.MENGER_SHELL_PORT;
} else {
  label = `port-${port}`;
  for (const name of Object.keys(ENVS)) {
    const v = readEnvFile(join(confDir, `${name}.env`));
    if (+v.MENGER_SHELL_PORT === port) {
      envVars = v;
      label = name;
    }
  }
}
const cred = (k) => process.env[k] || envVars[k] || '';
const base = `http://127.0.0.1:${port}`;

/** A target → { url, name, app, origin } — `app` runs the app's checks. */
function target(t) {
  if (t.startsWith('#')) return { url: `${base}/${t}`, name: `${label}.${slug(t)}`, app: true, origin: base };
  if (/^https?:\/\//i.test(t)) {
    const u = new URL(t);
    return { url: u.href, name: `${u.port || u.hostname}.${slug(u.hash || u.pathname)}`, app: !opt.page, origin: u.origin };
  }
  const path = resolve(t);
  if (!existsSync(path)) usage(`${t}: not a route (#/…), a URL, or a file that exists`);
  return { url: pathToFileURL(path).href, name: `file.${slug(basename(path, extname(path)))}`, app: false, origin: null };
}
function slug(s) {
  const out = s.replace(/^[#/]+/, '').replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '');
  return out || 'board';
}
const targets = parsed.positionals.map(target);

// ── Where the files go ────────────────────────────────────────────────────

const shots = targets.flatMap((t) => themes.flatMap((theme) => widths.map((width) => ({ t, theme, width }))));
let outDir = resolve(opt.out ?? join(tmpdir(), 'menger-shots'));
let single = null;
if (opt.out && opt.out.toLowerCase().endsWith('.png')) {
  if (shots.length > 1) usage(`--out ${opt.out}: a file, but this run takes ${shots.length} shots — give a directory`);
  single = outDir;
  outDir = dirname(outDir);
}
mkdirSync(outDir, { recursive: true });

// ── Playwright, from perf/node_modules beside this checkout ──────────────

let chromium;
try {
  ({ chromium } = createRequire(new URL('../perf/', import.meta.url))('playwright'));
} catch {
  console.error('shot: Playwright is not installed here — run (cd perf && npm ci) first');
  process.exit(1);
}

// ── In the page: what is still pending ───────────────────────────────────

// Every frame posts to its parent; the host page records which frames have
// said they are menger frames (`menger:ready`) and which have settled.
// Installed before any of the page's own scripts run.
function listen() {
  if (window !== window.top || window.__shot) return;
  const ready = new WeakSet();
  const settled = new WeakSet();
  window.__shot = { ready, settled };
  window.addEventListener(
    'message',
    (e) => {
      const d = e.data;
      if (!d || typeof d.type !== 'string' || !e.source) return;
      if (d.type === 'menger:ready') ready.add(e.source);
      if (d.type === 'menger:size' && d.settled) settled.add(e.source);
    },
    true,
  );
}

/** What the page is still waiting on (empty: ready), and its layout's shape. */
function probe({ app, waitFor }) {
  const pending = [];
  const errors = [];
  const main = document.querySelector('main');
  if (document.readyState !== 'complete') pending.push(`the document (${document.readyState})`);
  if (document.fonts && document.fonts.status !== 'loaded') pending.push('fonts in the page');
  const imgs = [...document.images].filter((i) => !i.complete);
  if (imgs.length) pending.push(`${imgs.length} image(s)`);
  if (waitFor && !document.querySelector(waitFor)) pending.push(`--wait-for ${waitFor} (nothing matches yet)`);
  if (app) {
    const conn = document.getElementById('conn');
    if (!conn) pending.push('the app (no #conn yet — still loading, or not the app: --page)');
    else if (!/^live\b/.test(conn.textContent.trim())) pending.push(`the socket (#conn reads "${conn.textContent.trim()}")`);
    // DESIGN-39: the boot's sheet removes itself (≤ 1.3 s after health).
    if (document.getElementById('boot')) pending.push('the boot sheet');
    const nav = document.getElementById('nav');
    if (nav && !nav.closest('.nonav') && !nav.querySelector('[data-nav-id], [data-node-key]')) pending.push('the nav');
    if (!main) pending.push('the route (no <main>)');
    else {
      const loading = [...main.querySelectorAll('.loading, p.none')]
        .map((el) => el.textContent.trim())
        .filter((s) => /^(loading|placing)\b/i.test(s));
      if (loading.length) pending.push(`the route (${loading.map((s) => `"${s}"`).join(', ')})`);
      if (![...main.children].some((el) => el.id !== 'health' && el.id !== 'error')) pending.push('the route (nothing drawn in <main>)');
    }
    const err = document.getElementById('error');
    if (err) errors.push(err.textContent.trim());
  }
  // A menger frame (the app's, or any page's that runs the SDK) is ready
  // once it has reported `settled`.
  const s = window.__shot;
  const frames = [...document.querySelectorAll('iframe')];
  const unsettled = frames.filter((f) => {
    const w = f.contentWindow;
    const menger = f.hasAttribute('data-channel') || (s && w && s.ready.has(w));
    return menger && !(s && w && s.settled.has(w));
  });
  if (unsettled.length) {
    const names = unsettled.map((f) => f.getAttribute('data-ticket') || f.getAttribute('title') || 'a frame');
    pending.push(`${unsettled.length} of ${frames.length} framed bodies to settle (${names.join(', ')})`);
  }
  // The layout's shape: if any of it moves, it has not finished drawing.
  const scrollers = [document.scrollingElement, ...document.querySelectorAll('#app .content, #nav .tree-scroll')].filter(Boolean);
  const shape = [
    document.getElementsByTagName('*').length,
    ...scrollers.map((el) => `${el.scrollWidth}x${el.scrollHeight}`),
    ...frames.map((f) => Math.round(f.getBoundingClientRect().height)),
    document.body ? document.body.textContent.length : 0,
    document.body ? Math.round(document.body.getBoundingClientRect().height) : 0,
  ].join('|');
  // How tall the page would be with nothing scrolled: the tallest scroller
  // plus the room above it.
  const tall = Math.max(
    ...scrollers.map((el) => el.scrollHeight + Math.max(0, el.getBoundingClientRect().top + window.scrollY)),
  );
  return { pending, errors, shape, tall };
}

/** Each child frame: its document loaded and its fonts in. */
async function framePending(page) {
  const out = [];
  for (const f of page.frames()) {
    if (f === page.mainFrame()) continue;
    try {
      const st = await f.evaluate(() => [document.readyState, document.fonts ? document.fonts.status : 'loaded']);
      if (st[0] !== 'complete') out.push(`a frame's document (${st[0]})`);
      if (st[1] !== 'loaded') out.push("a frame's fonts");
    } catch {
      // A frame swapped out while we asked (a re-render) — the next poll
      // sees its successor.
    }
  }
  return out;
}

const QUIET_MS = 600; // the layout unchanged this long counts as still
const POLL_MS = 100;

/** Polls until nothing is pending and the layout has held still; throws with
 *  what was still pending at the deadline. */
async function settle(page, app, deadline) {
  let last = null;
  let since = Date.now();
  let state = { pending: ['the first look'] };
  for (;;) {
    try {
      state = await page.evaluate(probe, { app, waitFor: opt['wait-for'] ?? null });
      state.pending.push(...(await framePending(page)));
    } catch (e) {
      // A navigation (the login redirect, a hash change) replaced the context.
      state = { pending: [`the page (${e.message.split('\n')[0]})`], shape: null };
    }
    const now = Date.now();
    if (state.shape !== last) {
      since = now;
      last = state.shape;
    }
    if (!state.pending.length && now - since >= QUIET_MS) return state;
    if (now > deadline) {
      const why = state.pending.length ? state.pending : [`the layout to hold still for ${QUIET_MS} ms (it kept moving)`];
      throw new Error(`not ready after ${Math.round(timeoutMs / 1000)} s — still waiting for: ${why.join('; ')}`);
    }
    await new Promise((r) => setTimeout(r, POLL_MS));
  }
}

// ── Signing in ────────────────────────────────────────────────────────────

const signedIn = new Set();
const warnedOpen = new Set();

/** Where the server sent the page to /login, sign the context in — the one
 *  thing this script submits. */
async function signIn(context, origin, next) {
  if (signedIn.has(origin)) throw new Error(`${origin} sent the page to /login again after signing in`);
  const headers = { origin }; // what the browser's own form would send
  let res;
  let who;
  if (opt.as) {
    who = `${opt.as} (the dev sign-in)`;
    res = await context.request.post(`${origin}/login/dev`, {
      headers,
      form: { user: opt.as, role: 'viewer', scope: '*', next },
      maxRedirects: 0,
    });
    if (res.status() === 404) throw new Error(`--as ${opt.as}: ${origin} does not offer the dev sign-in (/login/dev)`);
  } else {
    const user = cred('MENGER_SMOKE_USER');
    const password = cred('MENGER_SMOKE_PASSWORD');
    if (!user || !password) {
      throw new Error(
        `${origin} requires sign-in: set MENGER_SMOKE_USER and MENGER_SMOKE_PASSWORD (in ~/.config/menger/${label}.env or the environment), or pass --as USER`,
      );
    }
    who = `${user} (the smoke account)`;
    res = await context.request.post(`${origin}/login`, { headers, form: { username: user, password, next }, maxRedirects: 0 });
  }
  // A good sign-in answers 303 to `next` with the session cookie.
  const to = res.headers()['location'] || '';
  if (res.status() !== 303 || to.startsWith('/login')) {
    const body = await res.text().catch(() => '');
    const why = body.match(/class="error"[^>]*>([^<]*)</)?.[1]?.trim();
    throw new Error(`signing in as ${who} failed: HTTP ${res.status()}${why ? ` — ${why}` : ''}`);
  }
  const me = await context.request.get(`${origin}/login/me`);
  if (!me.ok()) throw new Error(`signed in as ${who}, but ${origin}/login/me answers ${me.status()}`);
  signedIn.add(origin);
  console.error(`shot: signed in to ${origin} as ${who}`);
}

// ── One shot ──────────────────────────────────────────────────────────────

async function shoot(context, { t, theme, width }) {
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message.split('\n')[0]));
  try {
    await page.emulateMedia({ colorScheme: theme, reducedMotion: opt['reduced-motion'] ? 'reduce' : 'no-preference' });
    await page.setViewportSize({ width, height });
    await page.goto(t.url, { waitUntil: 'domcontentloaded', timeout: timeoutMs });
    if (t.origin && new URL(page.url()).pathname.startsWith('/login')) {
      const u = new URL(t.url);
      await signIn(context, t.origin, u.pathname + u.search);
      await page.goto(t.url, { waitUntil: 'domcontentloaded', timeout: timeoutMs });
      if (new URL(page.url()).pathname.startsWith('/login')) throw new Error(`still at ${page.url()} after signing in`);
    }
    if (opt.as && t.origin && !signedIn.has(t.origin) && !warnedOpen.has(t.origin)) {
      warnedOpen.add(t.origin);
      console.error(`shot: ${t.origin} did not ask for sign-in — --as ${opt.as} unused`);
    }
    const deadline = Date.now() + timeoutMs;
    let state = await settle(page, t.app, deadline);
    // The whole page: the app scrolls inside `.content`, not the document,
    // so the window grows to the tallest scroller, and is settled again.
    if (!opt.screen) {
      for (let i = 0; i < 4 && state.tall > height; i++) {
        const want = Math.min(MAX_HEIGHT, Math.ceil(state.tall));
        const now = page.viewportSize().height;
        if (want <= now) break;
        await page.setViewportSize({ width, height: want });
        state = await settle(page, t.app, deadline);
      }
      if (state.tall > MAX_HEIGHT) console.error(`shot: ${t.name}: the page is ${state.tall}px tall — cut at ${MAX_HEIGHT}px`);
    }
    for (const e of state.errors) console.error(`shot: ${t.name}: the app shows an error: ${e}`);
    for (const e of errors) console.error(`shot: ${t.name}: page error: ${e}`);
    // DEPLOY-8: named for the theme the page painted — its own
    // `<html data-theme>` wins over the emulated colour scheme.
    const painted = await page.evaluate(() => {
      const d = document.documentElement.getAttribute('data-theme');
      return d === 'light' || d === 'dark' ? d : matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    });
    if (painted !== theme) {
      console.error(`shot: ${t.name}: asked for ${theme}, the page paints ${painted} (its own data-theme) — named ${painted}`);
      if (themes.length > 1) console.error(`shot: ${t.name}: --theme both gives the same theme twice here`);
    }
    const file = single ?? join(outDir, `${t.name}.${painted}.${width}.png`);
    if (opt.selector) {
      const el = page.locator(opt.selector).first();
      await el.waitFor({ state: 'visible', timeout: Math.max(1, deadline - Date.now()) }).catch(() => {
        throw new Error(`no visible element matches --selector ${opt.selector}`);
      });
      await el.screenshot({ path: file, animations: 'disabled' });
    } else {
      await page.screenshot({ path: file, fullPage: !opt.screen, animations: 'disabled' });
    }
    console.log(file);
    return true;
  } catch (e) {
    console.error(`shot: ${t.name} ${theme} ${width}: ${e.message.split('\n')[0]}`);
    for (const e of errors) console.error(`shot:   page error: ${e}`);
    return false;
  } finally {
    await page.close();
  }
}

const browser = await chromium.launch();
let ok = true;
try {
  const context = await browser.newContext({ deviceScaleFactor: 1 });
  await context.addInitScript(listen);
  for (const s of shots) ok = (await shoot(context, s)) && ok;
} finally {
  await browser.close();
}
process.exit(ok ? 0 : 1);
