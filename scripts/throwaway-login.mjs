#!/usr/bin/env node
// ACCESS-52: signing in to a THROWAWAY menger — the one way every script
// that starts one does it. ACCESS-24 took the open mode away: a server
// starts only with a login provider, and a client with no credential sees
// the anonymous view (auth.* and nothing else). So a script that starts a
// throwaway server
//
//   1. makes a key of its own in its scratch directory (never ~/.plexus):
//        node scripts/throwaway-login.mjs key "$work/operator.key"     → key:<hex>
//   2. starts menger with `--auth-provider key --admin key:<hex>` (and
//      `--no-rest`: a release from before ACCESS-24 would otherwise open
//      its REST bridge on prod's default port);
//   3. mints a session with that key — a key proof at the upgrade, then
//      `auth.login {flow: {session: {}}}` — and keeps its `mgs_` token in
//      a file (0600):
//        node scripts/throwaway-login.mjs session --url ws://127.0.0.1:$port \
//          --key "$work/operator.key" --out "$work/session" [--agent owner]
//   4. carries the token: synapse `--token-file "$work/session"` (before the
//      backend name; it rides the upgrade as `Cookie: access_token=…`),
//      synapse-cc `--token-file`, a WebSocket's `Authorization: Bearer …`
//      (bearer() below; Node's WebSocket takes `{ headers }`), a browser's
//      session cookie (sessionCookies() below: the shell's
//      `menger_session_<port>`, the one a sign-in at /login sets).
//
// A session is the agent's name (`--agent`, default `owner`): a comment or
// a note it writes is signed with that name. The key and the token are a
// throwaway server's, in its scratch directory, gone with it; they never
// ride argv or the environment — only the files' paths do
// (MENGER_SESSION_FILE names one for the perf probes).
//
// Node ≥ 22, its standard library only (deploy/menger-call.mjs's socket).
import crypto from "node:crypto";
import fs from "node:fs";
import { pathToFileURL } from "node:url";
import { call, connect, keyProof, publicHex, seedOf } from "../deploy/menger-call.mjs";

/** A fresh Ed25519 seed in `file` (base64, 0600; an existing one is kept),
 *  and its principal, `key:<hex>`. */
export function makeKey(file) {
  try {
    fs.writeFileSync(file, crypto.randomBytes(32).toString("base64") + "\n", { mode: 0o600, flag: "wx" });
  } catch (e) {
    if (e.code !== "EEXIST") throw e;
  }
  return principalOf(file);
}

/** The principal of the key in `file`. */
export function principalOf(file) {
  return `key:${publicHex(seedOf(fs.readFileSync(file, "utf8")))}`;
}

/** A session on the hub at `url` for the key in `keyFile`, as `agent`:
 *  `{ token, principal, name, agent, expires_at }`. */
export async function mintSession(url, keyFile, agent = "owner", ttlSecs) {
  const aud = url.replace(/\/+$/, "");
  const seed = seedOf(fs.readFileSync(keyFile, "utf8"));
  const ws = await connect(url, { Authorization: `Bearer ${keyProof(seed, aud, agent)}` });
  let session = null, refusal = null;
  try {
    const flow = { session: ttlSecs ? { ttl_secs: ttlSecs } : {} };
    await call(ws, 1, "auth.login", { flow }, (item) => {
      if (item?.type === "data" && item.content?.type === "session") session = item.content;
      else if (item?.type === "data" || item?.type === "error") refusal = item.content ?? item;
    });
  } finally {
    ws.close();
  }
  if (!session?.token) throw new Error(`auth.login at ${url} answered no session${refusal ? `: ${JSON.stringify(refusal)}` : ""}`);
  return session;
}

/** mintSession, its token written to `out` (0600). */
export async function writeSession(url, keyFile, out, agent = "owner") {
  const s = await mintSession(url, keyFile, agent);
  fs.rmSync(out, { force: true });
  fs.writeFileSync(out, s.token + "\n", { mode: 0o600, flag: "wx" });
  return s;
}

/** The token a session file holds. */
export function readToken(file) {
  const t = fs.readFileSync(file, "utf8").trim();
  if (!t) throw new Error(`${file}: no session token`);
  return t;
}

/** The upgrade's headers for the session in `file` (default
 *  $MENGER_SESSION_FILE): `{ Authorization: "Bearer mgs_…" }`, or `{}`
 *  when there is no file to read. */
export function bearer(file = process.env.MENGER_SESSION_FILE) {
  return file ? { Authorization: `Bearer ${readToken(file)}` } : {};
}

/** The browser's sign-in, for a Playwright context: the session cookie the
 *  shell at `origin` (http://127.0.0.1:PORT) reads — over http
 *  `menger_session_<port>` (plexus-login's names: the port is in the
 *  name), as a sign-in at /login sets it. */
export function sessionCookies(origin, token) {
  const u = new URL(origin);
  const name = u.protocol === "https:" ? "__Host-menger_session" : `menger_session_${u.port || 80}`;
  return [{ name, value: token, url: `${u.protocol}//${u.host}/`, httpOnly: true, sameSite: "Lax" }];
}

/** Signs a Playwright context in to the shell at `origin` with `token`. */
export async function signInContext(context, origin, token) {
  await context.addCookies(sessionCookies(origin, token));
}

// ── the command line ─────────────────────────────────────────────────────
const die = (code, msg) => { process.stderr.write(`throwaway-login: ${msg}\n`); process.exit(code); };
async function main(argv) {
  const [cmd, ...rest] = argv;
  const opt = {};
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i];
    if (!a.startsWith("--")) { (opt._ ??= []).push(a); continue; }
    opt[a.slice(2)] = rest[++i] ?? die(1, `${a} needs a value`);
  }
  if (cmd === "key") {
    const file = opt._?.[0] ?? die(1, "usage: throwaway-login.mjs key FILE");
    process.stdout.write(makeKey(file) + "\n");
  } else if (cmd === "principal") {
    process.stdout.write(principalOf(opt._?.[0] ?? die(1, "usage: throwaway-login.mjs principal FILE")) + "\n");
  } else if (cmd === "session") {
    if (!opt.url || !opt.key || !opt.out) die(1, "usage: throwaway-login.mjs session --url ws://127.0.0.1:PORT --key FILE --out FILE [--agent NAME]");
    const s = await writeSession(opt.url, opt.key, opt.out, opt.agent ?? "owner");
    process.stdout.write(`${s.principal} ${s.name ?? s.agent}\n`);
  } else {
    die(1, "usage: throwaway-login.mjs key FILE | principal FILE | session --url URL --key FILE --out FILE [--agent NAME]");
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(fs.realpathSync(process.argv[1])).href) {
  main(process.argv.slice(2)).catch((e) => die(3, e.message));
}
