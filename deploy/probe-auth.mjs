// IDENTITY-38: the login's adversarial probe — run against the TEST
// environment after a deploy, it plays the attacks the 2026-10-10 review
// found (IDENTITY-24's successors: IDENTITY-36, 37, 38) against the live
// server, and fails if any of them works.
//
//   node deploy/probe-auth.mjs [ws port] [shell port]      (default 4450 4451)
//
// It signs in through the dev sign-in (/login/dev, test only), so it never
// runs against prod. It writes to test's database — a probe ticket in each
// of two shortcodes, a grant it then revokes — which the next
// `deploy/pipeline deploy test` or `sync test` replaces with prod's.
// Shortcodes: PROBE_HOME (default MENGER) and PROBE_AWAY (default
// IDENTITY), two that host neither the other.
const [wsPort = '4450', shellPort = '4451'] = process.argv.slice(2);
const SHELL = `http://127.0.0.1:${shellPort}`;
const HOME = process.env.PROBE_HOME || 'MENGER';
const AWAY = process.env.PROBE_AWAY || 'IDENTITY';
const WANT_COOKIE = `menger_session_${shellPort}`;
let COOKIE = WANT_COOKIE; // what the server actually sets, learnt at the first sign-in

let failed = 0;
const ok = (what) => console.log(`ok   ${what}`);
const fail = (what) => { console.log(`FAIL ${what}`); failed++; };
const check = (cond, what) => (cond ? ok(what) : fail(what));

// ── the wire ─────────────────────────────────────────────────────────────

/** Opens a socket; resolves to it, or to the upgrade's refusal. */
function open({ origin = SHELL, session } = {}) {
  const headers = {};
  if (origin) headers.origin = origin;
  if (session) headers.cookie = `${COOKIE}=${session}`;
  return new Promise((resolve) => {
    const ws = new WebSocket(`ws://127.0.0.1:${wsPort}`, { headers });
    ws.onopen = () => resolve({ ws });
    ws.onerror = (e) => resolve({ refused: String(e?.message || e?.error?.message || 'refused') });
  });
}

let nextId = 1;
/** Calls `method`; resolves to its data items, or { error }. */
function call(ws, method, params = {}, { ms = 4000 } = {}) {
  const id = nextId++;
  return new Promise((resolve) => {
    const items = [];
    let sub;
    const done = (r) => { clearTimeout(t); ws.removeEventListener('message', on); resolve(r); };
    const t = setTimeout(() => done(items), ms);
    const on = (m) => {
      const d = JSON.parse(m.data);
      if (d.id === id) {
        if (d.error) done({ error: d.error.message });
        else sub = d.result;
        return;
      }
      const r = d.params?.result;
      if (!r || d.params.subscription !== sub) return;
      if (r.type === 'data') items.push(r.content);
      else if (r.type === 'error') items.push({ type: 'error', message: r.message });
      else if (r.type === 'done') done(items);
    };
    ws.addEventListener('message', on);
    ws.send(JSON.stringify({ jsonrpc: '2.0', id, method: 'menger.call', params: { method, params } }));
  });
}

/** A watch: collects what it hears until stopped. */
function watch(ws) {
  const id = nextId++;
  const heard = [];
  let sub;
  ws.addEventListener('message', (m) => {
    const d = JSON.parse(m.data);
    if (d.id === id) { sub = d.result; return; }
    const r = d.params?.result;
    if (r && d.params.subscription === sub && r.type === 'data') heard.push(r.content);
  });
  ws.send(JSON.stringify({ jsonrpc: '2.0', id, method: 'menger.call', params: { method: 'tickets.watch', params: {} } }));
  return heard;
}

const errorOf = (items) => (Array.isArray(items) ? items.find((i) => i.type === 'error')?.message : items.error) || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Signs in through the dev sign-in; the session id. */
async function signIn(user, role, scope) {
  const form = new URLSearchParams({ user, next: '/' });
  if (role) { form.set('role', role); form.set('scope', scope || '*'); }
  const r = await fetch(`${SHELL}/login/dev`, { method: 'POST', body: form, redirect: 'manual', headers: { origin: SHELL } });
  const set = r.headers.getSetCookie().find((c) => c.startsWith('menger_session'));
  if (!set) throw new Error(`dev sign-in as ${user}: no session cookie (status ${r.status}) — is this test?`);
  COOKIE = set.split('=')[0];
  return set.slice(COOKIE.length + 1).split(';')[0];
}

// ── the probe ────────────────────────────────────────────────────────────

const tag = `probe-${Date.now().toString(36)}`;

// The door.
check((await open({ origin: null })).refused, 'an anonymous upgrade is refused');
check((await open({ origin: 'https://evil.example' })).refused, 'a foreign page\'s upgrade is refused');
check((await open({ origin: 'null' })).refused, 'a sandboxed frame\'s upgrade (Origin null) is refused');
check((await open({ origin: null, session: 'forged' })).refused, 'a forged session is refused');

// The pages.
for (const path of ['/', '/login']) {
  const r = await fetch(`${SHELL}${path}`, { redirect: 'manual' });
  const csp = r.headers.get('content-security-policy') || '';
  check(r.headers.get('x-frame-options') === 'DENY' && csp.includes("frame-ancestors 'none'"), `${path} refuses to be framed`);
}
const lure = await (await fetch(`${SHELL}/login?error=${encodeURIComponent('sign in at evil.example')}`)).text();
check(!lure.includes('evil.example'), 'the login page does not show a link\'s ?error= text');
const foreign = await fetch(`${SHELL}/login/dev`, { method: 'POST', body: new URLSearchParams({ user: 'x' }), redirect: 'manual', headers: { origin: 'https://evil.example' } });
check(foreign.status === 403, 'a sign-in POST from another page is refused');

// The people.
const admin = await signIn('admin');
const nobody = await signIn(`${tag}-nobody`);
const viewer = await signIn(`${tag}-viewer`, 'viewer', HOME);
const editor = await signIn(`${tag}-editor`, 'editor', AWAY);
check(COOKIE === WANT_COOKIE, `the session cookie is this shell's own, ${WANT_COOKIE} (IDENTITY-38; got ${COOKIE})`);
const a = (await open({ session: admin })).ws;
const n = (await open({ session: nobody })).ws;
const v = (await open({ session: viewer })).ws;
const e = (await open({ session: editor })).ws;
check(a && n && v && e, 'each signed-in person opens a socket');

// A ticket in each shortcode, made by the admin; the away one blocked by home's.
const made = async (epic, extra = {}) => (await call(a, 'tickets.create', { epic, title: `${tag} ${epic}`, body: `${tag} secret body of ${epic}`, ...extra }))
  .find((i) => i.ticket)?.ticket?.id;
const homeId = await made(HOME);
const awayId = await made(AWAY, { blocked_by: homeId ? [homeId] : [] });
check(homeId && awayId, `the admin makes a probe ticket in ${HOME} and ${AWAY}`);

// Reads.
check(errorOf(await call(n, 'tickets.list', {})).includes('no access'), 'a person with no grant reads nothing');
const vGet = await call(v, 'tickets.get', { id: awayId });
check(!JSON.stringify(vGet).includes(`secret body of ${AWAY}`), `a viewer of ${HOME} cannot get a ${AWAY} ticket`);
const vSearch = await call(v, 'tickets.search', { query: tag });
check(!JSON.stringify(vSearch).includes(`secret body of ${AWAY}`), `a viewer of ${HOME} finds no ${AWAY} ticket by search`);
const eBlockers = await call(e, 'tickets.blockers', { id: awayId });
check(!JSON.stringify(eBlockers).includes(`secret body of ${HOME}`), `an editor of ${AWAY} cannot read a ${HOME} blocker through blockers (IDENTITY-37)`);
const sneak = await call(e, 'tickets.create', { epic: AWAY, title: `${tag} sneak`, blocked_by: [homeId] });
check(errorOf(sneak), `an editor of ${AWAY} cannot name a ${HOME} ticket it cannot view as a blocker (IDENTITY-37)`);

// Writes.
check(errorOf(await call(v, 'tickets.set_status', { id: homeId, status: 'Ready' })), 'a viewer cannot set a status');
check(errorOf(await call(e, 'tickets.set_status', { id: homeId, status: 'Ready' })), `an editor of ${AWAY} cannot write in ${HOME}`);
check(errorOf(await call(e, 'tickets.create', { epic: HOME, title: `${tag} x` })), `an editor of ${AWAY} cannot create in ${HOME}`);
const me = (await call(e, 'session.whoami')).find((i) => i.principal)?.principal;
check(errorOf(await call(e, 'members.grant', { principal: me, scope: '*', role: 'admin' })), 'an editor cannot grant itself admin on *');
check(errorOf(await call(e, 'members.grant', { principal: me, scope: AWAY, role: 'admin' })), `an editor cannot grant itself admin on ${AWAY}`);
check(errorOf(await call(e, 'members.requests')), 'only an admin of * sees who has signed in (IDENTITY-37)');

// A watch follows the grants as they stand (IDENTITY-37).
const vWho = (await call(v, 'session.whoami')).find((i) => i.principal)?.principal;
const w = (await open({ session: viewer })).ws;
const heard = watch(w);
await sleep(800);
await call(a, 'members.revoke', { principal: vWho, scope: HOME });
await sleep(300);
await call(a, 'tickets.amend', { id: homeId, body: `${tag} secret body of ${HOME}, after the revoke` });
await sleep(1200);
check(!JSON.stringify(heard).includes('after the revoke'), 'a watch hears nothing once its grant is revoked (IDENTITY-37)');

// Sign-out ends the session's sockets.
await fetch(`${SHELL}/logout`, { method: 'POST', redirect: 'manual', headers: { origin: SHELL, cookie: `${COOKIE}=${nobody}` } });
check(errorOf(await call(n, 'tickets.health')).includes('signed out'), 'a socket stops answering when its session signs out');
check((await open({ session: nobody })).refused, 'a signed-out session opens no socket');

for (const s of [a, n, v, e, w]) s?.close();
console.log(failed ? `${failed} check(s) FAILED` : 'every attack was refused');
process.exit(failed ? 1 : 0);
