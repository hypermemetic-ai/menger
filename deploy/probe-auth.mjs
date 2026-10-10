// IDENTITY-38: the login's adversarial probe — run against the TEST
// environment after a deploy, it plays the attacks the 2026-10-10 review
// found (IDENTITY-24's successors: IDENTITY-36, 37, 38) against the live
// server, and fails if any of them works.
//
// AUTH-7: menger speaks the AUTH protocol — an anonymous socket is admitted
// into the anonymous view (only auth.providers, auth.login, auth.signup),
// every call passes the gate, a refusal is typed (error_kind, grant, scope,
// roles, held_on), each caller's schema lists only what it holds. The probe
// checks those too; its role_set check edits `viewer` and puts it back.
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
function open({ origin = SHELL, session, bearer } = {}) {
  const headers = {};
  if (origin) headers.origin = origin;
  if (session) headers.cookie = `${COOKIE}=${session}`;
  if (bearer) headers.authorization = `Bearer ${bearer}`;
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
      else if (r.type === 'error') {
        // AUTH-7: the gate's refusal is the stream's error item, typed.
        const { message, error_kind, grant, scope, roles, held_on } = r;
        items.push({ type: 'error', message, error_kind, grant, scope, roles, held_on });
        done(items);
      } else if (r.type === 'done') done(items);
    };
    ws.addEventListener('message', on);
    ws.send(JSON.stringify({ jsonrpc: '2.0', id, method: 'menger.call', params: { method, params } }));
  });
}

/** AUTH-4: the connection's `_info`. */
function info(ws) {
  const id = nextId++;
  return new Promise((resolve) => {
    const t = setTimeout(() => { ws.removeEventListener('message', on); resolve({}); }, 4000);
    let sub;
    const on = (m) => {
      const d = JSON.parse(m.data);
      if (d.id === id) { sub = d.result; return; }
      const r = d.params?.result;
      if (r && d.params.subscription === sub && r.type === 'data') {
        clearTimeout(t);
        ws.removeEventListener('message', on);
        resolve(r.content);
      }
    };
    ws.addEventListener('message', on);
    ws.send(JSON.stringify({ jsonrpc: '2.0', id, method: '_info', params: {} }));
  });
}

/** AUTH-4: the activations a connection's view shows, each with its methods. */
async function viewOf(ws) {
  const hub = (await call(ws, 'menger.schema'))[0] || {};
  const out = {};
  for (const c of hub.children || []) {
    const s = (await call(ws, `${c.namespace}.schema`))[0] || {};
    out[c.namespace] = (s.methods || []).map((m) => m.name).filter((n) => n !== 'schema').sort();
  }
  return out;
}

/** The refusal an answer carries, when the gate typed it. */
const refusalOf = (items) => (Array.isArray(items) ? items.find((i) => i.type === 'error' && i.error_kind) : null) || {};

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

// The door. AUTH-7: an anonymous upgrade is admitted — into the anonymous
// view, which shows only how to sign in and reaches nothing else.
const DOOR = ['auth.providers', 'auth.login', 'auth.signup'];
const anon = (await open({ origin: null })).ws;
check(anon, 'an anonymous upgrade is admitted into the anonymous view (AUTH-6)');
if (anon) {
  const i = await info(anon);
  check(i.view === 'anonymous' && JSON.stringify(i.auth?.methods) === JSON.stringify(DOOR), `an anonymous _info names only the door: ${DOOR.join(', ')}`);
  const v = await viewOf(anon);
  check(JSON.stringify(v) === JSON.stringify({ auth: ['login', 'providers', 'signup'] }), `an anonymous schema shows only the auth door (${JSON.stringify(v)})`);
  let reached = [];
  for (const m of ['tickets.health', 'tickets.list', 'comments.list', 'members.mine', 'session.whoami', 'auth.whoami', 'auth.roles']) {
    if (refusalOf(await call(anon, m)).error_kind !== 'unauthenticated') reached.push(m);
  }
  check(!reached.length, `an anonymous connection can call nothing but the door${reached.length ? ` (reached ${reached.join(', ')})` : ''}`);
  check((await call(anon, 'auth.providers')).some((p) => p.type === 'provider'), 'auth.providers answers it');
  anon.close();
}
check((await open({ origin: 'https://evil.example' })).refused, 'a foreign page\'s upgrade is refused');
check((await open({ origin: 'null' })).refused, 'a sandboxed frame\'s upgrade (Origin null) is refused');
check((await open({ origin: null, bearer: 'mgs_forged' })).refused, 'a forged session token is refused at the upgrade');
{
  // A forged browser cookie is no credential: the anonymous view, never anyone.
  const f = (await open({ origin: null, session: 'forged' })).ws;
  check(f && refusalOf(await call(f, 'session.whoami')).error_kind === 'unauthenticated', 'a forged session cookie proves no one');
  f?.close();
}

// The pages.
for (const path of ['/', '/login']) {
  const r = await fetch(`${SHELL}${path}`, { redirect: 'manual' });
  const csp = r.headers.get('content-security-policy') || '';
  check(r.headers.get('x-frame-options') === 'DENY' && csp.includes("frame-ancestors 'none'"), `${path} refuses to be framed`);
}
const lure = await (await fetch(`${SHELL}/login?error=${encodeURIComponent('sign in at evil.example')}`)).text();
check(!lure.includes('evil.example'), 'the login page does not show a link\'s ?error= text');
const vendored = await fetch(`${SHELL}/app/vendor/mermaid-11.17.2.min.js`, { redirect: 'manual' });
check(vendored.status === 200, 'a frame loads the vendored mermaid without a session (frames send none)');
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

// Reads. AUTH-7: a person with no grant is refused by the gate, typed.
{
  const r = refusalOf(await call(n, 'tickets.list', {}));
  check(r.error_kind === 'forbidden' && r.grant === 'tickets.list' && r.scope === '*' && (r.held_on || []).length === 0,
    `a person with no grant reads nothing: a typed refusal (${JSON.stringify(r)})`);
}
const vGet = await call(v, 'tickets.get', { id: awayId });
check(!JSON.stringify(vGet).includes(`secret body of ${AWAY}`), `a viewer of ${HOME} cannot get a ${AWAY} ticket`);
const vSearch = await call(v, 'tickets.search', { query: tag });
check(!JSON.stringify(vSearch).includes(`secret body of ${AWAY}`), `a viewer of ${HOME} finds no ${AWAY} ticket by search`);
const eBlockers = await call(e, 'tickets.blockers', { id: awayId });
check(!JSON.stringify(eBlockers).includes(`secret body of ${HOME}`), `an editor of ${AWAY} cannot read a ${HOME} blocker through blockers (IDENTITY-37)`);
const sneak = await call(e, 'tickets.create', { epic: AWAY, title: `${tag} sneak`, blocked_by: [homeId] });
check(errorOf(sneak), `an editor of ${AWAY} cannot name a ${HOME} ticket it cannot view as a blocker (IDENTITY-37)`);

// Writes.
{
  const r = refusalOf(await call(v, 'tickets.set_status', { id: homeId, status: 'Ready' }));
  check(r.error_kind === 'forbidden' && r.grant === 'tickets.set_status' && r.scope === HOME
    && (r.roles || []).includes('editor') && (r.roles || []).includes('admin') && Array.isArray(r.held_on),
    `a viewer cannot set a status: refused, typed — grant, scope, roles, held_on (${JSON.stringify(r)})`);
}
check(errorOf(await call(e, 'tickets.set_status', { id: homeId, status: 'Ready' })), `an editor of ${AWAY} cannot write in ${HOME}`);
check(errorOf(await call(e, 'tickets.create', { epic: HOME, title: `${tag} x` })), `an editor of ${AWAY} cannot create in ${HOME}`);
const me = (await call(e, 'session.whoami')).find((i) => i.principal)?.principal;
check(errorOf(await call(e, 'members.grant', { principal: me, scope: '*', role: 'admin' })), 'an editor cannot grant itself admin on *');
check(errorOf(await call(e, 'members.grant', { principal: me, scope: AWAY, role: 'admin' })), `an editor cannot grant itself admin on ${AWAY}`);
check(errorOf(await call(e, 'members.requests')), 'only an admin of * sees who has signed in (IDENTITY-37)');

// AUTH-4/AUTH-7: each caller's view is what it holds.
{
  const nv = await viewOf(n);
  check(!nv.tickets && !nv.comments && JSON.stringify(nv.members) === '["mine"]', `a person holding nothing sees no tickets or comments method (${JSON.stringify(nv)})`);
  const vv = await viewOf(v);
  check(vv.tickets?.includes('list') && !vv.tickets?.includes('set_status') && !vv.members?.includes('grant'),
    'a viewer\'s schema lists the reads, not the writes or the admin\'s methods');
  const ev = await viewOf(e);
  check(ev.tickets?.includes('set_status') && !ev.members?.includes('grant'), 'an editor\'s schema lists the writes, not the admin\'s methods');
  const vs = (await call(v, 'tickets.schema'))[0];
  const list = (vs?.methods || []).find((m) => m.name === 'list');
  check(list?.grant === 'tickets.list' && JSON.stringify(list?.held_on) === JSON.stringify([HOME]), `a method in a view says its grant and where it is held (${JSON.stringify(list?.held_on)})`);
  const hashes = await Promise.all([a, n, v, e].map(async (s) => (await info(s)).view_hash));
  check(hashes.every(Boolean) && new Set(hashes).size === 4, `view hashes differ by holdings (${hashes.join(', ')})`);
  // who am I, what does this need.
  const me = (await call(v, 'auth.whoami'))[0] || {};
  check(me.type === 'whoami' && (me.memberships || []).some((m) => m.role === 'viewer' && m.scope === HOME), `auth.whoami says the viewer's membership (${JSON.stringify(me.memberships)})`);
  const req = (await call(v, 'auth.requires', { method: 'tickets.set_status' }))[0] || {};
  check(req.grant === 'tickets.set_status' && req.target === 'id' && (req.roles || []).includes('editor'), `auth.requires says what set_status needs (${JSON.stringify(req)})`);
  check(refusalOf(await call(v, 'auth.role_set', { role: 'viewer', grants: ['tickets.get'] })).grant === 'auth.role_set', 'a viewer cannot edit a role');
  // An admin's role_set moves a caller's view — then viewer is put back.
  const before = (await call(a, 'auth.roles')).find((r) => r.role === 'viewer');
  if (!before?.grants?.length) {
    fail('auth.roles lists viewer\'s grants');
  } else {
    const set = (await call(a, 'auth.role_set', { role: 'viewer', grants: ['tickets.get'] }))[0];
    const narrowed = await viewOf(v);
    const listRefused = refusalOf(await call(v, 'tickets.list')).grant === 'tickets.list';
    const back = (await call(a, 'auth.role_set', { role: 'viewer', grants: before.grants }))[0];
    check(set?.type === 'role' && JSON.stringify(narrowed.tickets) === '["get"]' && listRefused, `an admin's auth.role_set narrows a viewer's view and calls (${JSON.stringify(narrowed.tickets)})`);
    check(JSON.stringify(back?.grants) === JSON.stringify(before.grants), 'viewer is put back as it was');
  }
}

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

// IDENTITY-42: an agent attests with an Ed25519 key (where test takes key
// proofs); its session holds at most the key's grants, narrower if asked.
{
  const { generateKeyPairSync, sign, randomBytes } = await import('node:crypto');
  const b64u = (b) => Buffer.from(b).toString('base64url');
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  const proof = (aud, agent = `${tag}-agent`) => {
    const nonce = b64u(randomBytes(32)), ts = Math.floor(Date.now() / 1000);
    const msg = `plexus-key-login/v1\n${aud}\n${agent}\n${nonce}\n${ts}`;
    const sig = b64u(sign(null, Buffer.from(msg), privateKey));
    return 'plexus-key.' + b64u(JSON.stringify({ v: 1, key: hex, aud, agent, nonce, ts, sig }));
  };
  const bearer = (token) => new Promise((resolve) => {
    const ws = new WebSocket(`ws://127.0.0.1:${wsPort}`, { headers: { authorization: `Bearer ${token}` } });
    ws.onopen = () => resolve({ ws });
    ws.onerror = () => resolve({ refused: true });
  });
  const aud = `ws://127.0.0.1:${wsPort}`;
  const first = await bearer(proof(aud));
  if (first.refused) {
    console.log(`skip agent checks: :${wsPort} takes no key proofs (--auth-provider key)`);
  } else {
    // The key holds editor on both shortcodes; a session asks for HOME only.
    await call(a, 'members.grant', { principal: `key:${hex}`, scope: HOME, role: 'editor' });
    await call(a, 'members.grant', { principal: `key:${hex}`, scope: AWAY, role: 'editor' });
    const who = (await call(first.ws, 'session.whoami')).find((i) => i.principal);
    check(who?.principal === `key:${hex}` && who?.name === `${tag}-agent`, 'a key proof opens the socket as key:<hex>, named by its agent');
    const login = (await call(first.ws, 'session.login', { scope: [`editor@${HOME}`] })).find((i) => i.token);
    check(login?.token?.startsWith('mgs_'), 'session.login mints a session token');
    const s = login && (await bearer(login.token)).ws;
    check(s && !errorOf(await call(s, 'tickets.set_status', { id: homeId, status: 'Pending' })), `the scoped session writes in ${HOME}`);
    check(s && errorOf(await call(s, 'tickets.set_status', { id: awayId, status: 'Pending' })), `the scoped session cannot write in ${AWAY}, though the key can`);
    check(s && errorOf(await call(s, 'members.grant', { principal: `key:${hex}`, scope: '*', role: 'admin' })), 'the session cannot grant its key more');
    const replayed = proof(aud);
    await bearer(replayed);
    check((await bearer(replayed)).refused, 'a key proof works once');
    check((await bearer(proof('ws://127.0.0.1:4455'))).refused, 'a key proof for another server is refused');
    for (const x of [first.ws, s]) x?.close();
  }
}

// AUTH-11 (AUTH-10 #1): a session that signed out holds nothing — the gate
// asks whether the sign-in stands on every method, auth.* included. The
// review's attack: an admin's session logs out, then the same socket
// grants a stranger admin on *. Then the server closes that socket.
{
  const { randomBytes } = await import('node:crypto');
  const stranger = `key:${randomBytes(32).toString('hex')}`;
  const doomed = (await open({ session: await signIn('admin') })).ws;
  const before = (await call(doomed, 'auth.whoami'))[0] || {};
  check(before.type === 'whoami' && (before.memberships || []).some((m) => m.role === 'admin'), 'a second admin session signs in (AUTH-11)');
  await call(doomed, 'auth.logout');
  const granted = await call(doomed, 'auth.grant', { principal: stranger, role: 'admin', scope: '*' });
  const r = refusalOf(granted);
  check(doomed.readyState !== WebSocket.OPEN || (r.error_kind === 'unauthenticated' && r.message?.includes('signed out')),
    `after auth.logout the same socket cannot auth.grant admin on * (AUTH-11: ${JSON.stringify(r)})`);
  // Nothing landed: the admin finds no role of the stranger's on * to
  // revoke (asked before the calls below, which could revoke it themselves).
  const landed = (await call(a, 'auth.revoke', { principal: stranger, scope: '*' }))[0] || {};
  check(!(landed.roles || []).length, `the stranger holds nothing (AUTH-11: ${JSON.stringify(landed)})`);
  const after = [];
  // Harmless if they ran: viewer re-set to the grants it has, the stranger's
  // (absent) roles revoked.
  const viewerNow = (await call(a, 'auth.roles')).find((x) => x.role === 'viewer')?.grants || [];
  for (const m of ['auth.role_set', 'auth.revoke', 'auth.waiting', 'tickets.list']) {
    if (doomed.readyState !== WebSocket.OPEN) break;
    const params = m === 'auth.role_set' ? { role: 'viewer', grants: viewerNow } : m === 'auth.revoke' ? { principal: stranger, scope: '*' } : {};
    if (refusalOf(await call(doomed, m, params)).error_kind !== 'unauthenticated') after.push(m);
  }
  check(!after.length, `a signed-out socket reaches no auth.* or service method (AUTH-11${after.length ? `: reached ${after.join(', ')}` : ''})`);
  for (let i = 0; i < 30 && doomed.readyState === WebSocket.OPEN; i++) await sleep(100);
  check(doomed.readyState !== WebSocket.OPEN, 'the server closes a socket whose session signed out (AUTH-11)');
}

// Sign-out ends the session's sockets. (AUTH-7: members.mine, which any
// signed-in caller reaches — `nobody` holds nothing the gate would let by.)
// AUTH-11: the server closes them, too — a call may find the socket gone.
await fetch(`${SHELL}/logout`, { method: 'POST', redirect: 'manual', headers: { origin: SHELL, cookie: `${COOKIE}=${nobody}` } });
check(n.readyState !== WebSocket.OPEN || errorOf(await call(n, 'members.mine')).includes('signed out'), 'a socket stops answering when its session signs out');
for (let i = 0; i < 30 && n.readyState === WebSocket.OPEN; i++) await sleep(100);
check(n.readyState !== WebSocket.OPEN, 'the server closes the signed-out session\'s socket (AUTH-11)');
{
  // AUTH-7: the ended session's cookie is no credential: the anonymous view.
  const o = await open({ session: nobody });
  check(o.refused || refusalOf(await call(o.ws, 'session.whoami')).error_kind === 'unauthenticated', 'a signed-out session opens no socket as anyone');
  o.ws?.close();
}

for (const s of [a, n, v, e, w]) s?.close();
console.log(failed ? `${failed} check(s) FAILED` : 'every attack was refused');
process.exit(failed ? 1 : 0);
