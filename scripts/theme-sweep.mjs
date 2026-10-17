#!/usr/bin/env node
// THEME-7: the sweep test — change the theme and nothing is left in the old
// one's colours, on any open page, inside frames and shadow roots, with no
// reload.
//
// It runs against a THROWAWAY server (never 4450–4458: it writes tickets and
// theme settings), seeds what it needs there (--seed), then opens every
// route at once — the board, a framed body, a mockup, a full-screen ticket,
// the wave boards, #/components and a docs card, #/inputs, #/comments,
// #/settings, and pages holding a dialog (the palette, the cheat sheet,
// MENGER-14's leave dialog), a menu and a toast — and drives one cause at a
// time from a page of its own:
//
//   default → pantry (the palette's `theme pantry`) → garish (the palette)
//   → garish dark (the light/dark toggle) → garish light → the site's
//   (the palette) → ocean (`themes set --scope site`, over watch) → ocean
//   with its --accent amended (its ```theme block, over watch: every page
//   is a "second tab") → default (the site's setting cleared)
//
// After each switch every page is scanned — every element of the document,
// of every open shadow root and of every frame: its computed color,
// background, border colours, outline, fill, stroke and font-family — and
// none may hold a value the old theme painted that the new one does not.
// No page may reload and no frame may be replaced (each is marked before
// the switch and must still carry its mark). A diagram must be redrawn in
// the new theme's --diagram-* colours. Then (--rows) every row of THEME-8's
// table is checked region by region, and (--signin-shell) the sign-in page
// is shown to wear the site theme and follow it live. THEME-4's walk
// (--project): #/board → a project's ticket → back repaints twice, the
// shell's and the frame's tokens the project's each time, no reload; a
// hosted shortcode inherits until it says its own; a reader's "project
// themes off" (settings, in another tab) puts the site's back.
//
// Usage (a throwaway server first — README "The sweep test"; ACCESS-52: it
// runs `--auth-provider key --admin` a key of its own, and the sweep signs
// in with that key — synapse and every browser page alike):
//   node scripts/theme-sweep.mjs --ws 4510 --shell 4511 --key /tmp/sweep.key --seed
//   node scripts/theme-sweep.mjs --ws 4510 --shell 4511 --key /tmp/sweep.key --browsers chromium,firefox --rows --project
//
// Exit 0 when every check held, 1 when one failed, 2 on usage. Playwright
// comes from perf/node_modules ((cd perf && npm ci)).
import { createRequire } from 'node:module';
import { mintSession, signInContext } from './throwaway-login.mjs';
import { parseArgs } from 'node:util';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

const HELP = `usage: scripts/theme-sweep.mjs --ws PORT --shell PORT --key FILE [options]

  --ws PORT            the throwaway server's WebSocket port (synapse -P)
  --shell PORT         its shell port (the app)
  --key FILE           the key the server runs as --admin (ACCESS-52:
                       node scripts/throwaway-login.mjs key FILE makes one
                       and prints its principal); the sweep signs in with it
  --seed               create the sweep's tickets and themes (idempotent)
  --epic CODE          the shortcode the sweep's tickets live in   [SWEEP]
  --browsers LIST      chromium,firefox                            [chromium]
  --rows               also every row of THEME-8's table
  --project            also THEME-4's walk into a project and out, and a
                       hosted shortcode inheriting
  --project-ticket ID  walk into this ticket's shortcode (e.g. PANTRY-4)
                       instead of the sweep's own project
  --signin-shell PORT  also the sign-in page on a server with login
  --out DIR            screenshots and the report      [$TMPDIR/theme-sweep]
  --headed             show the browsers
  -h, --help           this text

Never against 4450–4458 (prod and test): it writes.`;

const usage = (m) => {
  console.error(`theme-sweep: ${m}\n${HELP}`);
  process.exit(2);
};
let parsed;
try {
  parsed = parseArgs({
    options: {
      ws: { type: 'string' },
      shell: { type: 'string' },
      key: { type: 'string' },
      seed: { type: 'boolean', default: false },
      epic: { type: 'string', default: 'SWEEP' },
      browsers: { type: 'string', default: 'chromium' },
      rows: { type: 'boolean', default: false },
      project: { type: 'boolean', default: false },
      'project-ticket': { type: 'string' },
      'signin-shell': { type: 'string' },
      out: { type: 'string' },
      headed: { type: 'boolean', default: false },
      help: { type: 'boolean', short: 'h', default: false },
    },
  });
} catch (e) {
  usage(e.message);
}
const opt = parsed.values;
if (opt.help) {
  console.log(HELP);
  process.exit(0);
}
const port = (name) => {
  const v = opt[name];
  if (!v || !/^\d+$/.test(v)) usage(`--${name}: a port`);
  if (+v >= 4450 && +v <= 4458) usage(`--${name} ${v}: that is prod's or test's — a throwaway server only`);
  return +v;
};
const WS = port('ws');
const SHELL = port('shell');
const SIGNIN = opt['signin-shell'] ? port('signin-shell') : null;
const EPIC = opt.epic.toUpperCase();
const OUT = resolve(opt.out ?? join(tmpdir(), 'theme-sweep'));
mkdirSync(OUT, { recursive: true });
const BASE = `http://127.0.0.1:${SHELL}`;
// ACCESS-24/52: no open mode — signed in as the server's --admin, a session
// minted with its key: synapse carries it as --token-file, every page as the
// shell's session cookie.
if (!opt.key) usage('--key: the key the throwaway server runs as --admin');
const SESSION = join(OUT, 'session');
const TOKEN = await (async () => {
  try {
    const s = await mintSession(`ws://127.0.0.1:${WS}`, opt.key, 'sweep');
    rmSync(SESSION, { force: true });
    writeFileSync(SESSION, s.token + '\n', { mode: 0o600, flag: 'wx' });
    return s.token;
  } catch (e) {
    console.error(`theme-sweep: could not sign in to :${WS} with ${opt.key}: ${e.message}`);
    process.exit(1);
  }
})();

const pw = (() => {
  try {
    return createRequire(new URL('../perf/', import.meta.url))('playwright');
  } catch {
    console.error('theme-sweep: Playwright is not installed here — run (cd perf && npm ci) first');
    process.exit(1);
  }
})();

// ── The token contract, read from the file every token comes from ──────

const CONTRACT = readFileSync(new URL('../tickets-api/src/tokens.toml', import.meta.url), 'utf8');
const TOKENS = [];
{
  let group = '';
  for (const line of CONTRACT.split('\n')) {
    const g = line.match(/^\[(.+)\]$/);
    if (g) group = g[1];
    const t = line.match(/^"(--[a-z0-9-]+)" = \['([^']*)', '([^']*)'/);
    if (t) TOKENS.push({ name: t[1], group, light: t[2], dark: t[3] });
  }
}
const COLOURS = TOKENS.filter((t) => t.group.startsWith('colour.'));
const FACES = TOKENS.filter((t) => t.group === 'type.face');

// ── synapse against the throwaway server ─────────────────────────────────

function synapse(args) {
  const out = execFileSync('synapse', ['--token-file', SESSION, '-j', '-P', String(WS), 'menger', ...args], { encoding: 'utf8', maxBuffer: 64 << 20 });
  return out
    .split('\n')
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .filter((x) => x.type === 'data')
    .map((x) => x.content);
}
/** CHAT-4: the sweep's chat's other participant — a fixed key, never used to sign in. */
const SWEEP_PEER = 'key:5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed';

function one(args) {
  const items = synapse(args);
  const err = items.find((i) => i.type === 'error');
  if (err) throw new Error(`synapse ${args.join(' ')}: ${err.message}`);
  return items[0];
}

// ── What the sweep wears: three themes, as ```theme blocks ──────────────

const PANTRY = `\`\`\`theme pantry
@import url("https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,600&family=Inter:wght@400;600&display=swap");
:root {
  --bg: #fbfaf7; --panel: #f3efe7; --ink: #1d1b18; --accent: #d4572a;
  --font-display: "Fraunces", serif;
  --font-text: "Inter", sans-serif;
  --radius: 6px; --radius-lg: 10px;
  --code-keyword: #b2401c;
}
:root[data-theme="dark"] { --bg: #171614; --panel: #1f1d1a; --ink: #efebe4; --accent: #f08a5d; }
m-ring::part(ring) { outline: 2px solid var(--accent); outline-offset: 2px; }
\`\`\``;

const OCEAN = (accent) => `\`\`\`theme ocean
:root { --accent: ${accent}; --bg: #f2f8fb; --panel: #e6f1f6; --ink: #0d2a36; --line: #c4dbe4; }
:root[data-theme="dark"] { --accent: #6fd3c7; --bg: #0b1a20; --panel: #10242c; --ink: #d4eef2; --line: #1d3a44; }
\`\`\``;

/** Every colour token a loud colour of its own, light and dark; every face
 * another family: nothing of default's is left in it. */
function garish() {
  const hex = (h, s, l) => {
    l /= 100;
    const a = (s * Math.min(l, 1 - l)) / 100;
    const f = (n) => {
      const k = (n + h / 30) % 12;
      return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))))
        .toString(16)
        .padStart(2, '0');
    };
    return `#${f(0)}${f(8)}${f(4)}`;
  };
  const light = [];
  const dark = [];
  COLOURS.forEach((t, i) => {
    const h = (i * 137) % 360;
    light.push(`${t.name}: ${hex(h, 95, 42 + (i % 5) * 4)};`);
    dark.push(`${t.name}: ${hex(h, 90, 62 + (i % 4) * 4)};`);
  });
  const faces = {
    '--mono': '"Courier New", Courier, monospace',
    '--sans': '"Comic Sans MS", "Comic Neue", cursive',
    '--font-text': 'Georgia, "Times New Roman", serif',
    '--font-display': 'Impact, "Arial Black", fantasy',
  };
  for (const f of FACES) light.push(`${f.name}: ${faces[f.name] ?? 'fantasy'};`);
  return `\`\`\`theme garish
:root { ${light.join(' ')} --radius: 0px; }
:root[data-theme="dark"] { ${dark.join(' ')} }
\`\`\``;
}

// ── Seeding: the sweep's tickets, idempotent ─────────────────────────────

const FRAMED_BODY = (code) => `<m-field data-field="what">

**The sweep's framed body.** Every surface a frame draws: prose, a [link out](https://example.com/sweep), code, a diagram, house components.

\`\`\`rust
fn main() { let n: u32 = 42; println!("{n}"); } // a comment
\`\`\`

\`\`\`mermaid
flowchart LR
  A[theme picked] --> B((repaint)) --> C[every surface]
\`\`\`

</m-field>

<m-ring id="${code}-1" size="24"></m-ring>

<m-decision data-field="decision" id="sweep">
<m-question data-field="question">Does every surface follow?</m-question>
<m-option data-field="option" name="Yes" value="yes" recommended cost="none" benefit="one look"></m-option>
<m-option data-field="option" name="No" value="no" cost="stale colours" benefit="none"></m-option>
</m-decision>`;

const MOCKUP_BODY = `---
view: mockup
widths: 390, 1024
---
<m-field data-field="what">

**A mockup.** Its devices' chrome is the app's; its pages are framed.

</m-field>`;

// THEME-8's table: what the ticket says (frontmatter) and the site's theme.
const ROWS = [
  { site: null, fm: '', want: ['default', 'default', 'default'] },
  { site: 'ocean', fm: '', want: ['ocean', 'ocean', 'ocean'] },
  { site: 'ocean', fm: 'takeover: off', want: ['ocean', 'default', 'default'] },
  { site: 'ocean', fm: 'frame-takeover: off', want: ['ocean', 'ocean', 'default'] },
  { site: null, fm: 'theme: pantry', want: ['default', 'pantry', 'pantry'] },
  { site: null, fm: 'theme: pantry\nreach: site', want: ['pantry', 'pantry', 'pantry'] },
  { site: null, fm: 'frame-theme: pantry', want: ['default', 'default', 'pantry'] },
  { site: 'ocean', fm: 'theme: pantry\nframe-takeover: off', want: ['ocean', 'pantry', 'default'] },
];

function seed() {
  const themes = new Map(synapse(['themes', 'list']).filter((e) => e.type === 'theme').map((e) => [e.theme.name, e.theme]));
  const have = synapse(['tickets', 'list', '--epic', EPIC]).filter((e) => e.type === 'ticket').map((e) => e.ticket);
  const byTitle = new Map(have.map((t) => [t.title, t.id]));
  const make = (title, kind, body, extra = []) => {
    if (byTitle.has(title)) return byTitle.get(title);
    const args = ['tickets', 'create', '--epic', EPIC, '--title', title, '--kind', kind, ...extra];
    if (body) args.push('--body', body);
    const ev = one(args);
    const id = ev.ticket.id;
    byTitle.set(title, id);
    console.log(`  seeded ${id}  ${title}`);
    return id;
  };
  make('Theme sweep', 'scope', 'The sweep test\'s tickets (THEME-7): scripts/theme-sweep.mjs.');
  if (!themes.has('pantry') || themes.get('pantry').owner?.startsWith(EPIC)) make('Sweep theme: pantry', 'doc', `The pantry look, for the sweep.\n\n${PANTRY}`);
  make('Sweep theme: garish', 'doc', `Every token loud, so nothing of another theme can hide.\n\n${garish()}`);
  if (!themes.has('ocean') || themes.get('ocean').owner?.startsWith(EPIC)) make('Sweep theme: ocean', 'doc', `A second site theme.\n\n${OCEAN('#00796b')}`);
  make('Sweep: a framed body', 'build', FRAMED_BODY(EPIC));
  make('Sweep: a mockup', 'build', MOCKUP_BODY);
  ROWS.forEach((r, i) => make(`Sweep row ${i + 1}`, 'build', `${r.fm ? `---\n${r.fm}\n---\n` : ''}${FRAMED_BODY(EPIC)}`));
  return byTitle;
}

function ids() {
  const have = synapse(['tickets', 'list', '--epic', EPIC]).filter((e) => e.type === 'ticket').map((e) => e.ticket);
  return new Map(have.map((t) => [t.title, t.id]));
}

// ── In every document: what it paints, scanned ───────────────────────────

/** Runs in a document (the app's or a frame's): every element's painted
 * values that are in `stale` (values normalised here), outside diagrams. */
function scanDoc(arg) {
  const { stale, region } = arg;
  const probe = document.createElement('div');
  (document.body || document.documentElement).appendChild(probe);
  const norm = (prop, v) => {
    probe.style.setProperty(prop, '');
    probe.style.setProperty(prop, v);
    return getComputedStyle(probe).getPropertyValue(prop);
  };
  const sets = {};
  for (const r of Object.keys(stale)) {
    // THEME-6: a value the new theme also paints is not stale, however it is
    // written (`#fff` in one, `#ffffff` in the other): compared normalised.
    // (only what is a colour, or a face, at all: an unparsable value would read back as the inherited one)
    const keepC = new Set((stale[r].keep || []).filter((v) => CSS.supports('color', v)).map((v) => norm('color', v)));
    const keepF = new Set((stale[r].keep || []).filter((v) => CSS.supports('font-family', v) && !CSS.supports('color', v)).map((v) => norm('font-family', v)));
    sets[r] = {
      colours: new Set(stale[r].colours.map((v) => norm('color', v)).filter((v) => v && v !== 'rgba(0, 0, 0, 0)' && !keepC.has(v))),
      faces: new Set(stale[r].faces.map((v) => norm('font-family', v)).filter((v) => !keepF.has(v))),
    };
  }
  probe.remove();
  const PROPS = ['color', 'background-color', 'border-top-color', 'border-right-color', 'border-bottom-color', 'border-left-color', 'outline-color', 'fill', 'stroke'];
  const found = [];
  let n = 0;
  const regionOf = (el) => {
    if (region) return region;
    const host = (e) => (e.getRootNode && e.getRootNode().host) || null;
    for (let e = el; e; e = e.parentElement || host(e)) {
      if (e.classList && e.classList.contains('mscope-frame')) return 'frame';
      if (e.id === 'ticket-page') return 'ticket';
    }
    return 'site';
  };
  const describe = (el) => {
    let s = el.tagName.toLowerCase();
    if (el.id) s += `#${el.id}`;
    else if (el.classList && el.classList.length) s += `.${[...el.classList].slice(0, 2).join('.')}`;
    return s;
  };
  const path = (el) => {
    const out = [];
    for (let e = el; e && out.length < 4; e = e.parentElement || ((e.getRootNode && e.getRootNode().host) || null)) out.unshift(describe(e));
    return out.join(' > ');
  };
  const walk = (root) => {
    for (const el of root.querySelectorAll('*')) {
      if (el.closest('.mermaid svg, svg[id^="menger-mermaid"], svg[aria-roledescription]')) continue;
      if (el.id === 'sweep-probe') continue;
      n++;
      const set = sets[regionOf(el)];
      if (!set) continue;
      const cs = getComputedStyle(el);
      for (const p of PROPS) {
        const v = cs.getPropertyValue(p);
        if (set.colours.has(v) && found.length < 300) found.push(`${path(el)} ${p}: ${v}`);
      }
      const f = cs.getPropertyValue('font-family');
      if (set.faces.has(f) && found.length < 300) found.push(`${path(el)} font-family: ${f}`);
      if (el.shadowRoot) walk(el.shadowRoot);
    }
  };
  walk(document);
  return { n, found };
}

/** The tokens painted at each region of the app's document now. */
function regionTokens(names) {
  const read = (el) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    const o = {};
    for (const n of names) o[n] = cs.getPropertyValue(n).trim();
    return o;
  };
  return {
    site: read(document.documentElement),
    ticket: read(document.getElementById('ticket-page')),
    frame: read(document.querySelector('.mscope-frame')),
    worn: {
      site: document.documentElement.getAttribute('data-theme-site'),
      ticket: document.documentElement.getAttribute('data-theme-ticket'),
      frame: document.documentElement.getAttribute('data-theme-frame'),
    },
  };
}

/** What `a` painted that `b` does not: per region, colours and faces. */
function staleOf(a, b) {
  const out = {};
  for (const r of ['site', 'ticket', 'frame']) {
    const A = a[r] ?? a.site;
    const B = b[r] ?? b.site;
    if (!A || !B) continue;
    const bVals = new Set(Object.values(B));
    const colours = [];
    const faces = [];
    for (const [n, v] of Object.entries(A)) {
      if (!v || bVals.has(v) || v === 'auto') continue;
      if (FACES.some((f) => f.name === n)) faces.push(v);
      else if (COLOURS.some((c) => c.name === n)) colours.push(v);
    }
    out[r] = { colours, faces, keep: [...bVals].filter((v) => v && v !== 'auto') };
  }
  return out;
}

// ── Pages ────────────────────────────────────────────────────────────────

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function ready(page) {
  await page.waitForFunction(() => /live/.test(document.getElementById('conn')?.textContent || ''), null, { timeout: 30000 });
  await page.waitForFunction(() => !/loading…/.test(document.querySelector('main')?.textContent || ''), null, { timeout: 30000 }).catch(() => {});
  await sleep(600);
}

async function frameDocs(page) {
  return page.frames().filter((f) => f !== page.mainFrame());
}

/** Marks a page and its frames: a reload or a replaced frame loses it. */
async function mark(page, n) {
  await page.evaluate((n) => {
    window.__sweepMark = n;
    document.querySelectorAll('iframe').forEach((f) => f.setAttribute('data-sweep-mark', String(n)));
  }, n);
  for (const f of await frameDocs(page)) await f.evaluate((n) => (window.__sweepMark = n), n).catch(() => {});
}
async function marked(page, n) {
  const problems = [];
  const main = await page.evaluate(() => window.__sweepMark).catch(() => undefined);
  if (main !== n) problems.push('the page reloaded');
  const unmarked = await page.evaluate((n) => [...document.querySelectorAll('iframe')].filter((f) => f.getAttribute('data-sweep-mark') !== String(n)).length, n);
  if (unmarked) problems.push(`${unmarked} frame(s) replaced`);
  for (const f of await frameDocs(page)) {
    const m = await f.evaluate(() => window.__sweepMark).catch(() => 'gone');
    if (m !== n && m !== 'gone') problems.push(`a frame's document reloaded (${f.url().slice(0, 40)})`);
  }
  return problems;
}

/** Every document of `page` scanned against `stale`. */
async function scanPage(page, stale) {
  const found = [];
  let n = 0;
  const main = await page.evaluate(scanDoc, { stale, region: null });
  n += main.n;
  for (const x of main.found.slice(0, 200)) found.push(x);
  for (const f of await frameDocs(page)) {
    const r = await f.evaluate(scanDoc, { stale, region: 'frame' }).catch(() => null);
    if (!r) continue;
    n += r.n;
    for (const x of r.found.slice(0, 200)) found.push(`[frame] ${x}`);
  }
  return { n, found };
}

/** The diagrams painted in `node` (null: any) — app and frames. */
async function diagramFills(page) {
  const read = () =>
    [...document.querySelectorAll('svg[id^="menger-mermaid"] .node rect, svg[id^="menger-mermaid"] .node circle, svg[aria-roledescription] .node rect')].map(
      (n) => getComputedStyle(n).fill,
    );
  const out = await page.evaluate(read);
  for (const f of await frameDocs(page)) out.push(...((await f.evaluate(read).catch(() => [])) || []));
  return out;
}

// ── The run ──────────────────────────────────────────────────────────────

const results = [];
function check(browser, step, what, ok, detail = '') {
  results.push({ browser, step, what, ok, detail });
  console.log(`${ok ? 'ok  ' : 'FAIL'} [${browser}] ${step}: ${what}${detail ? ` — ${detail}` : ''}`);
}

async function sweep(browserName, idsBy) {
  const framed = idsBy.get('Sweep: a framed body');
  const mockup = idsBy.get('Sweep: a mockup');
  const browser = await pw[browserName].launch({ headless: !opt.headed });
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, colorScheme: 'light' });
  await signInContext(ctx, BASE, TOKEN);
  // A clean reader: no pick, no toggle, before any page loads.
  synapse(['themes', 'set', '--scope', 'site']);
  const pages = {};
  const open = async (name, hash, then) => {
    const p = await ctx.newPage();
    await p.goto(`${BASE}/${hash}`);
    await ready(p);
    if (then) await then(p);
    pages[name] = p;
  };
  const driver = await ctx.newPage();
  await driver.goto(`${BASE}/#/settings`);
  await ready(driver);
  await driver.evaluate(() => {
    localStorage.removeItem('menger.themes.reader');
    localStorage.removeItem('menger.theme');
  });
  await driver.reload();
  await ready(driver);

  await open('board', '#/');
  await open('framed', `#/ticket/${framed}`);
  await open('mockup', `#/ticket/${mockup}`);
  await open('full', `#/ticket/${framed}?view=full`);
  // CHAT-4: the chat pane, open on a chat of the framed ticket. The peer is
  // a fixed key, never signed in; a chat takes only those who can read the
  // ticket (ACCESS-24: there is no login off), so it is a viewer of EPIC.
  one(['members', 'grant', '--principal', SWEEP_PEER, '--scope', EPIC, '--role', 'viewer']);
  const chat =one(['chats', 'open', '--ticket', framed, '--others', SWEEP_PEER]).chat.id;
  if (!synapse(['chats', 'messages', '--chat', String(chat)]).some((e) => e.type === 'message')) {
    one(['chats', 'post', '--chat', String(chat), '--body', 'The pane follows the theme: `code`, **bold**, a [link](#/).']);
  }
  await open('chat', `#/ticket/${framed}?chat=${chat}`);
  await open('waves', `#/waves/${EPIC}`);
  await open('waves-to', `#/waves/to/${framed}`);
  await open('components', '#/components');
  await open('docs-card', '#/components/m-field');
  await open('inputs', '#/inputs');
  await open('comments', '#/comments');
  await open('settings', '#/settings');
  await open('palette', '#/', async (p) => {
    await p.keyboard.press('Control+k');
    await p.waitForSelector('#palette');
  });
  await open('cheats', '#/', async (p) => {
    await p.keyboard.press('?');
    await p.waitForSelector('#cheats');
  });
  await open('menu', '#/', async (p) => {
    await p.locator('[data-ticket-id]').first().click({ button: 'right' });
    await p.waitForSelector('[role="menu"]');
  });
  await open('leave', `#/ticket/${framed}`, async (p) => {
    const fr = p.frameLocator('iframe.mframe-doc').first();
    await fr.locator('a[href^="https://example.com"]').first().click();
    await p.waitForSelector('#leave');
  });
  await open('toast', '#/');

  const NAMES = TOKENS.map((t) => t.name);
  let before = await driver.evaluate(regionTokens, NAMES);
  let markN = 0;
  const pick = (name) => async () => {
    await driver.keyboard.press('Control+k');
    await driver.waitForSelector('#palette-input');
    await driver.fill('#palette-input', `theme ${name}`);
    await driver.waitForSelector('.pal-cmd');
    await driver.keyboard.press('Enter');
  };
  const toggle = async () => {
    await driver.click('#theme');
  };
  // The amended accent: whichever ocean is not wearing now (runs repeat).
  const oceanNow = synapse(['themes', 'get', '--name', 'ocean']).find((e) => e.type === 'theme')?.theme?.light?.['--accent'];
  const amended = oceanNow === '#c2185b' ? '#00796b' : '#c2185b';
  const steps = [
    { name: 'pantry (palette)', cause: pick('pantry'), worn: 'pantry' },
    { name: 'garish (palette)', cause: pick('garish'), worn: 'garish', diagrams: true },
    { name: 'garish dark (toggle)', cause: async () => { await toggle(); await toggle(); }, worn: 'garish', scheme: 'dark' },
    { name: 'garish light (toggle)', cause: toggle, worn: 'garish', scheme: 'light' },
    { name: "the site's (palette)", cause: pick('site'), worn: 'default' },
    { name: 'ocean (site setting, watch)', cause: async () => synapse(['themes', 'set', '--scope', 'site', '--theme', 'ocean']), worn: 'ocean' },
    {
      name: "ocean's --accent amended (watch: every page a second tab)",
      cause: async () => {
        const id = idsBy.get('Sweep theme: ocean');
        if (id) one(['tickets', 'amend', '--id', id, '--body', `A second site theme.\n\n${OCEAN(amended)}`]);
      },
      worn: 'ocean',
      accent: amended,
    },
    { name: 'default (site setting cleared)', cause: async () => synapse(['themes', 'set', '--scope', 'site']), worn: 'default' },
  ];
  for (const step of steps) {
    markN++;
    for (const p of Object.values(pages)) await mark(p, markN);
    // The toast is short-lived: shown just before the switch.
    const tp = pages.toast;
    await tp.locator('[data-ticket-id]').first().click({ button: 'right' }).catch(() => {});
    await tp.locator('[role="menu"] [role="menuitem"]', { hasText: /copy/i }).first().click().catch(() => {});
    // THEME-7's SDK: a component's on('theme') listener, in the framed body.
    const lf = (await frameDocs(pages.framed))[0];
    await lf?.evaluate(() => {
      window.__themeCalls = [];
      if (window.__themeOff) window.__themeOff();
      window.__themeOff = window.menger.on('theme', (t) => window.__themeCalls.push({ t, accent: getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() }));
    });
    const t0 = Date.now();
    await step.cause();
    // Converged: the driver wears the new theme; then every page is scanned
    // until nothing stale is left (or 6 s).
    await driver
      .waitForFunction((w) => (document.documentElement.getAttribute('data-theme-site') || '').startsWith(w + ' '), step.worn, { timeout: 8000 })
      .catch(() => {});
    if (step.accent) {
      await driver
        .waitForFunction((a) => getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() === a, step.accent, { timeout: 8000 })
        .catch(() => {});
    }
    const after = await driver.evaluate(regionTokens, NAMES);
    const stale = staleOf(before, after);
    const nStale = stale.site.colours.length + stale.site.faces.length;
    let left = null;
    let ms = 0;
    for (let tries = 0; tries < 40; tries++) {
      left = {};
      let any = false;
      for (const [name, p] of Object.entries(pages)) {
        const r = await scanPage(p, stale);
        left[name] = r;
        if (r.found.length) any = true;
      }
      ms = Date.now() - t0;
      if (!any) break;
      await sleep(150);
    }
    const worn = (await driver.evaluate(() => document.documentElement.getAttribute('data-theme-site'))) || '';
    check(browserName, step.name, `the driver wears ${step.worn}`, worn.startsWith(`${step.worn} `), worn);
    check(browserName, step.name, `${nStale} old values to be gone`, nStale > 0 || step.name.includes('light'), '');
    for (const [name, r] of Object.entries(left)) {
      check(browserName, step.name, `${name}: nothing of the old theme (${r.n} elements)`, r.found.length === 0, r.found.slice(0, 5).join(' | '));
    }
    console.log(`     converged in ${ms} ms`);
    for (const [name, p] of Object.entries(pages)) {
      const probs = await marked(p, markN);
      check(browserName, step.name, `${name}: no reload, no frame replaced`, probs.length === 0, probs.join('; '));
      // the toast and the leave dialog stay up: the surfaces were really there
    }
    // The control: an element painted in an old value IS found — the scan
    // can fail, so its passing means something.
    const old = stale.site.colours[0] ?? stale.site.faces[0];
    if (old) {
      const prop = stale.site.colours.length ? 'color' : 'font-family';
      await pages.board.evaluate(([prop, v]) => {
        const d = document.createElement('span');
        d.id = 'sweep-control';
        d.style.setProperty(prop, v);
        d.textContent = 'control';
        document.body.appendChild(d);
      }, [prop, old]);
      const r = await scanPage(pages.board, stale);
      await pages.board.evaluate(() => document.getElementById('sweep-control')?.remove());
      check(browserName, step.name, `control: a span painted ${old} is found`, r.found.some((f) => f.includes('sweep-control')), '');
      // …and inside a frame, in a component's shadow root.
      const fr = (await frameDocs(pages.framed))[0];
      if (fr) {
        const where = await fr.evaluate(([prop, v]) => {
          const host = [...document.querySelectorAll('*')].find((e) => e.shadowRoot);
          const d = document.createElement('span');
          d.id = 'sweep-control';
          d.style.setProperty(prop, v);
          (host ? host.shadowRoot : document.body).appendChild(d);
          return host ? host.tagName.toLowerCase() : 'body';
        }, [prop, old]);
        const rf = await scanPage(pages.framed, stale);
        await fr.evaluate(() => document.querySelectorAll('*').forEach((e) => (e.shadowRoot || e).querySelector?.('#sweep-control')?.remove()));
        await fr.evaluate(() => document.getElementById('sweep-control')?.remove());
        check(browserName, step.name, `control: one in a frame's ${where} shadow root is found`, rf.found.some((f) => f.includes('[frame]') && f.includes('sweep-control')), '');
      }
    }
    if (lf) {
      const calls = await lf.evaluate(() => window.__themeCalls);
      const last = calls[calls.length - 1];
      check(
        browserName,
        step.name,
        "on('theme') heard once, after the tokens were live",
        calls.length === 1 && last.t.theme === step.worn && last.t.tokens['--accent'] === last.accent && last.accent === after.site['--accent'],
        calls.map((c) => `${c.t.theme}/${c.t.scheme} ${c.t.tokens['--accent']} (live ${c.accent})`).join('; '),
      );
    }
    const toastUp = await tp.evaluate(() => !!document.getElementById('toast'));
    check(browserName, step.name, 'a toast was up through the switch', toastUp);
    if (step.diagrams) {
      const want = after.site['--diagram-node'];
      const fills = await diagramFills(pages.framed);
      const norm = await driver.evaluate((v) => {
        const d = document.createElement('div');
        d.style.color = v;
        document.body.appendChild(d);
        const c = getComputedStyle(d).color;
        d.remove();
        return c;
      }, want);
      check(browserName, step.name, `diagrams redrawn in --diagram-node ${want}`, fills.length > 0 && fills.every((f) => f === norm), `${fills.length} nodes: ${[...new Set(fills)].join(', ')}`);
    }
    await pages.framed.screenshot({ path: join(OUT, `${browserName}.${String(markN).padStart(2, '0')}.framed.png`) });
    await pages.board.screenshot({ path: join(OUT, `${browserName}.${String(markN).padStart(2, '0')}.board.png`) });
    before = after;
  }

  if (opt.rows) await rows(browserName, ctx, idsBy);
  if (opt.project) await project(browserName, ctx, idsBy);
  await browser.close();
}

/** THEME-8's table, region by region: the site, the ticket view, the
 * frame's box and the frame's own document. */
async function rows(browserName, ctx, idsBy) {
  const NAMES = TOKENS.map((t) => t.name);
  const page = await ctx.newPage();
  await page.goto(`${BASE}/#/`);
  await ready(page);
  const accentOf = {};
  for (const [i, row] of ROWS.entries()) {
    const id = idsBy.get(`Sweep row ${i + 1}`);
    synapse(row.site ? ['themes', 'set', '--scope', 'site', '--theme', row.site] : ['themes', 'set', '--scope', 'site']);
    const before = await page.evaluate(regionTokens, NAMES);
    await page.goto(`${BASE}/#/ticket/${id}`);
    await page.waitForFunction(
      (w) => {
        const r = document.documentElement;
        return (r.getAttribute('data-theme-site') || '').startsWith(w[0] + ' ') && (r.getAttribute('data-theme-ticket') || '').startsWith(w[1] + ' ') && (r.getAttribute('data-theme-frame') || '').startsWith(w[2] + ' ');
      },
      row.want,
      { timeout: 10000 },
    ).catch(() => {});
    await page.waitForSelector('iframe.mframe-doc', { timeout: 10000 }).catch(() => {});
    await sleep(800);
    const after = await page.evaluate(regionTokens, NAMES);
    const fr = (await frameDocs(page))[0];
    const inFrame = fr ? await fr.evaluate(() => ({ theme: window.menger && window.menger.theme && window.menger.theme.theme, accent: getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() })).catch(() => null) : null;
    const worn = ['site', 'ticket', 'frame'].map((r) => (after.worn[r] || '').split(' ')[0]);
    check(browserName, `row ${i + 1} (${row.site ?? 'default'} | ${row.fm.replace(/\n/g, ', ') || '—'})`, `wears ${row.want.join(' / ')}`, worn.join('/') === row.want.join('/'), worn.join(' / '));
    for (const t of ['default', 'ocean', 'pantry']) accentOf[t] ??= null;
    if (inFrame) {
      check(browserName, `row ${i + 1}`, `the frame's own document wears ${row.want[2]} (menger.theme)`, inFrame.theme === row.want[2], `${inFrame.theme} ${inFrame.accent}`);
      check(browserName, `row ${i + 1}`, "the frame's --accent is its box's", inFrame.accent === after.frame?.['--accent'], `${inFrame.accent} vs ${after.frame?.['--accent']}`);
    }
    const r = await scanPage(page, staleOf(before, after));
    check(browserName, `row ${i + 1}`, `nothing of the previous row's themes (${r.n} elements)`, r.found.length === 0, r.found.slice(0, 5).join(' | '));
  }
  synapse(['themes', 'set', '--scope', 'site']);
  await page.close();
}

/** THEME-4: #/board → a project's ticket → back, a hosted shortcode, and a
 * reader's "project themes off". */
async function project(browserName, ctx, idsBy) {
  const NAMES = TOKENS.map((t) => t.name);
  const sc = `${EPIC}P`;
  const hosted = `${EPIC}H`;
  // The sweep's project: its own shortcode, hosted under the sweep's root;
  // and a shortcode hosted under it.
  const list = (code) => synapse(['tickets', 'list', '--epic', code]).filter((e) => e.type === 'ticket').map((e) => e.ticket);
  let p = list(sc);
  if (!p.length) {
    const root = one(['tickets', 'create', '--epic', sc, '--title', 'A project', '--kind', 'scope', '--parent', idsBy.get('Theme sweep')]).ticket.id;
    one(['tickets', 'create', '--epic', sc, '--title', 'A ticket of the project', '--kind', 'build', '--body', FRAMED_BODY(EPIC), '--parent', root]);
    p = list(sc);
  }
  let h = list(hosted);
  if (!h.length) {
    const hroot = one(['tickets', 'create', '--epic', hosted, '--title', 'A hosted shortcode', '--kind', 'scope', '--parent', p.find((t) => t.kind === 'scope').id]).ticket.id;
    one(['tickets', 'create', '--epic', hosted, '--title', 'A ticket of the hosted shortcode', '--kind', 'build', '--body', FRAMED_BODY(EPIC), '--parent', hroot]);
    h = list(hosted);
  }
  const own = p.find((t) => t.kind !== 'scope').id;
  const walk = opt['project-ticket'] ? opt['project-ticket'].toUpperCase() : own;
  const walkSc = walk.replace(/-\d+$/, '');
  const hticket = h.find((t) => t.kind !== 'scope').id;
  synapse(['themes', 'set', '--scope', 'site']);
  synapse(['themes', 'set', '--scope', hosted]);
  synapse(['themes', 'set', '--scope', sc, '--theme', 'pantry', '--reach', 'site']);
  if (walkSc !== sc) synapse(['themes', 'set', '--scope', walkSc, '--theme', 'pantry', '--reach', 'site']);
  const step = `project ${walkSc}`;
  const siteIs = (w) => page.waitForFunction((w) => (document.documentElement.getAttribute('data-theme-site') || '').startsWith(w + ' '), w, { timeout: 10000 }).catch(() => {});

  const page = await ctx.newPage();
  await page.goto(`${BASE}/#/`);
  await ready(page);
  await page.evaluate(() => {
    window.__sweepMark = 'project';
    window.__repaints = [];
    new MutationObserver(() => window.__repaints.push(document.documentElement.getAttribute('data-theme-site'))).observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme-site'],
    });
  });
  const tokensAt = () => page.evaluate(regionTokens, NAMES);
  const frameAccent = async () => {
    const fr = (await frameDocs(page))[0];
    return fr ? fr.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()) : null;
  };
  const board = await tokensAt();
  check(browserName, step, 'the board wears the site theme', (board.worn.site || '').startsWith('default '), board.worn.site);
  // In, by the app's own navigation (no reload).
  await page.evaluate((id) => (location.hash = `#/ticket/${id}`), walk);
  await siteIs('pantry');
  await page.waitForSelector('iframe.mframe-doc', { timeout: 10000 }).catch(() => {});
  await sleep(1000);
  const inside = await tokensAt();
  const fa = await frameAccent();
  check(browserName, step, `in ${walk}: the shell wears pantry, from ${walkSc}`, (inside.worn.site || '').startsWith('pantry ') && /shortcode/.test(inside.worn.ticket || ''), `${inside.worn.site} / ${inside.worn.ticket}`);
  check(browserName, step, `in ${walk}: the frame's tokens are the shell's`, fa === null || fa === inside.site['--accent'], `${fa} vs ${inside.site['--accent']}`);
  let r = await scanPage(page, staleOf(board, inside));
  check(browserName, step, `in ${walk}: nothing of the site theme left (${r.n} elements)`, r.found.length === 0, r.found.slice(0, 5).join(' | '));
  await page.screenshot({ path: join(OUT, `${browserName}.project.in.png`) });
  // Out.
  await page.evaluate(() => (location.hash = '#/'));
  await siteIs('default');
  await sleep(600);
  const out = await tokensAt();
  check(browserName, step, 'back on the board: the site theme again', (out.worn.site || '').startsWith('default '), out.worn.site);
  r = await scanPage(page, staleOf(inside, out));
  check(browserName, step, `back on the board: nothing of pantry left (${r.n} elements)`, r.found.length === 0, r.found.slice(0, 5).join(' | '));
  const repaints = await page.evaluate(() => window.__repaints.filter((v, i, a) => i === 0 || v !== a[i - 1]));
  check(browserName, step, 'repainted twice (in, out)', repaints.length === 2, repaints.join(' → '));
  check(browserName, step, 'no reload', (await page.evaluate(() => window.__sweepMark)) === 'project');
  await page.screenshot({ path: join(OUT, `${browserName}.project.out.png`) });

  // A hosted shortcode inherits — painted at once, no detour by the site's.
  await page.evaluate((id) => (location.hash = `#/ticket/${id}`), own);
  await siteIs('pantry');
  await sleep(600);
  await page.evaluate(() => (window.__repaints = []));
  await page.evaluate((id) => (location.hash = `#/ticket/${id}`), hticket);
  await page.waitForSelector('iframe.mframe-doc', { timeout: 10000 }).catch(() => {});
  await sleep(1200);
  let w = await tokensAt();
  const detour = await page.evaluate(() => window.__repaints);
  check(browserName, 'hosted', `${hticket} (${hosted}, hosted under ${sc}) inherits pantry`, (w.worn.site || '').startsWith('pantry ') && /shortcode/.test(w.worn.ticket || ''), `${w.worn.site} / ${w.worn.ticket}`);
  check(browserName, 'hosted', `from ${own} to ${hticket}: no repaint through the site's`, detour.every((v) => (v || '').startsWith('pantry ')), detour.join(' → ') || 'none');
  synapse(['themes', 'set', '--scope', hosted, '--theme', 'ocean']);
  await page.waitForFunction(() => (document.documentElement.getAttribute('data-theme-ticket') || '').startsWith('ocean '), null, { timeout: 10000 }).catch(() => {});
  w = await tokensAt();
  check(browserName, 'hosted', `${hosted}'s own setting (ocean) overrides, live`, (w.worn.ticket || '').startsWith('ocean ') && (w.worn.site || '').startsWith('default '), `${w.worn.site} / ${w.worn.ticket}`);
  synapse(['themes', 'set', '--scope', hosted]);
  await siteIs('pantry');

  // A reader turns project themes off in settings, in another tab: the
  // project's page wears the site's, live.
  await page.evaluate((id) => (location.hash = `#/ticket/${id}`), walk);
  await siteIs('pantry');
  const other = await ctx.newPage();
  await other.goto(`${BASE}/#/settings`);
  await ready(other);
  await other.check('#theme-projects-off');
  await siteIs('default');
  w = await tokensAt();
  check(browserName, 'projects off', `${walk} wears the site's theme`, (w.worn.site || '').startsWith('default ') && (w.worn.ticket || '').startsWith('default '), `${w.worn.site} / ${w.worn.ticket}`);
  await other.uncheck('#theme-projects-off');
  await siteIs('pantry');
  w = await tokensAt();
  check(browserName, 'projects off', 'turned back on: pantry again', (w.worn.site || '').startsWith('pantry '), w.worn.site);
  await other.close();
  synapse(['themes', 'set', '--scope', sc]);
  if (walkSc !== sc) synapse(['themes', 'set', '--scope', walkSc]);
  await page.close();
}

/** The sign-in page: it wears the site theme the app last painted, and
 * follows another tab's change live. */
async function signin(browserName, garishCss) {
  const browser = await pw[browserName].launch({ headless: !opt.headed });
  const ctx = await browser.newContext({ colorScheme: 'light' });
  const base = `http://127.0.0.1:${SIGNIN}`;
  const a = await ctx.newPage();
  await a.goto(`${base}/login`);
  await a.evaluate(() => localStorage.removeItem('menger.themes.site'));
  await a.reload();
  const read = () => a.evaluate((n) => Object.fromEntries(n.map((t) => [t, getComputedStyle(document.documentElement).getPropertyValue(t).trim()])), TOKENS.map((t) => t.name));
  const before = await read();
  const b = await ctx.newPage();
  await b.goto(`${base}/login`);
  await b.evaluate((css) => localStorage.setItem('menger.themes.site', css), garishCss);
  await a.waitForFunction((v) => getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() !== v, before['--accent'], { timeout: 5000 }).catch(() => {});
  const after = await read();
  const r = await scanPage(a, staleOf({ site: before }, { site: after }));
  check(browserName, 'sign-in', 'follows the site theme live (another tab)', after['--accent'] !== before['--accent'] && r.found.length === 0, `${before['--accent']} → ${after['--accent']}; ${r.found.slice(0, 5).join(' | ')}`);
  await a.screenshot({ path: join(OUT, `${browserName}.signin.png`) });
  await a.reload();
  const again = await read();
  check(browserName, 'sign-in', 'wears it from the first paint after a reload', again['--accent'] === after['--accent'], again['--accent']);
  await browser.close();
}

// ── main ─────────────────────────────────────────────────────────────────

if (opt.seed) {
  console.log(`seeding ${EPIC} on :${WS}`);
  seed();
}
const idsBy = ids();
for (const need of ['Sweep: a framed body', 'Sweep: a mockup', 'Sweep theme: garish']) {
  if (!idsBy.has(need)) usage(`no "${need}" in ${EPIC} on :${WS} — run with --seed first`);
}
const browsers = opt.browsers.split(',').map((b) => b.trim()).filter(Boolean);
let garishCss = null;
for (const b of browsers) {
  if (!pw[b]) usage(`--browsers: ${b} is not one Playwright has`);
  try {
    await sweep(b, idsBy);
  } catch (e) {
    check(b, 'run', 'the sweep ran', false, e.message.split('\n')[0]);
    if (process.env.SWEEP_DEBUG) console.error(e);
  }
  if (SIGNIN) {
    if (!garishCss) {
      // What the app keeps for a garish site: read it from an app page.
      const br = await pw[b].launch({ headless: true });
      const pg = await br.newPage();
      await pg.goto(`${BASE}/#/settings`);
      await ready(pg);
      await pg.evaluate(() => localStorage.setItem('menger.themes.reader', JSON.stringify({ site: 'garish', takeovers_off: false })));
      await pg.reload();
      await ready(pg);
      await pg.waitForFunction(() => (document.documentElement.getAttribute('data-theme-site') || '').startsWith('garish '));
      garishCss = await pg.evaluate(() => localStorage.getItem('menger.themes.site'));
      await br.close();
    }
    try {
      await signin(b, garishCss);
    } catch (e) {
      check(b, 'sign-in', 'the sign-in check ran', false, e.message.split('\n')[0]);
    }
  }
}
const failed = results.filter((r) => !r.ok);
writeFileSync(join(OUT, 'report.json'), JSON.stringify(results, null, 2));
console.log(`\n${results.length - failed.length}/${results.length} checks held; report and screenshots in ${OUT}`);
process.exit(failed.length ? 1 : 0);
