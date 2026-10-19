#!/usr/bin/env node
// DEPLOY-40: a person's sign-in for a THROWAWAY menger — a loopback OIDC
// issuer (discovery and JWKS, as plexus-idp publishes them; tests/login.rs's
// fixtures, in Node) whose freshly made RSA key signs one token for a
// synthetic person. Started with `--auth-provider 'idp=<issuer>,plexus:menger'`,
// menger turns that token into `idp:<sub>` — a principal that is not a key,
// which a check needs where only a person may act (→ prod's runner refuses
// every key and agent).
//
//   node scripts/fixture-idp.mjs serve --port 4489 --sub 0d0e4e57-0000-4000-8000-00000000a1c0 --out DIR   (a sub is a UUID)
//       serves http://127.0.0.1:4489/ until stopped; writes DIR/idp.jwt (0600, 1 h)
//   node scripts/fixture-idp.mjs session --url ws://127.0.0.1:PORT --jwt DIR/idp.jwt --out DIR/owner.session
//       the token at the upgrade (`Authorization: Bearer`), then
//       auth.login {flow: {session: {}}}: the mgs_ token in a file (0600),
//       for scripts/throwaway-login.mjs's sessionCookies
//
// Synthetic and loopback only: the key lives in this process and dies with it.
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { call, connect } from "../deploy/menger-call.mjs";

const AUD = "plexus:menger";
const b64u = (b) => Buffer.from(b).toString("base64url");

function opts(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i += 2) o[argv[i].replace(/^--/, "")] = argv[i + 1];
  return o;
}

async function serve(o) {
  const port = Number(o.port || 4489), sub = o.sub || "0d0e4e57-0000-4000-8000-00000000a1c0", out = o.out || ".";
  const iss = `http://127.0.0.1:${port}/`;
  const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", { modulusLength: 2048 });
  const jwk = { ...publicKey.export({ format: "jwk" }), use: "sig", alg: "RS256", kid: "fixture-1" };
  const now = Math.floor(Date.now() / 1000);
  const head = b64u(JSON.stringify({ alg: "RS256", typ: "JWT", kid: "fixture-1" }));
  const claims = b64u(JSON.stringify({ iss, aud: AUD, sub, exp: now + 3600, iat: now - 5, username: o.name || "owner", roles: ["user"] }));
  const sig = crypto.sign("sha256", Buffer.from(`${head}.${claims}`), privateKey);
  const file = path.join(out, "idp.jwt");
  fs.rmSync(file, { force: true });
  fs.writeFileSync(file, `${head}.${claims}.${b64u(sig)}\n`, { mode: 0o600, flag: "wx" });
  http.createServer((req, res) => {
    const json = (v) => { res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify(v)); };
    if (req.url === "/.well-known/openid-configuration") return json({ issuer: iss, jwks_uri: `${iss}jwks.json` });
    if (req.url === "/jwks.json") return json({ keys: [jwk] });
    res.writeHead(404); res.end();
  }).listen(port, "127.0.0.1", () => console.log(`fixture-idp: ${iss} (sub ${sub}); token in ${file}`));
}

async function session(o) {
  const jwt = fs.readFileSync(o.jwt, "utf8").trim();
  const ws = await connect(o.url, { Authorization: `Bearer ${jwt}` });
  let s = null, refusal = null;
  try {
    await call(ws, 1, "auth.login", { flow: { session: {} } }, (item) => {
      if (item?.type === "data" && item.content?.type === "session") s = item.content;
      else if (item?.type === "data" || item?.type === "error") refusal = item.content ?? item;
    });
  } finally { ws.close(); }
  if (!s?.token) throw new Error(`auth.login answered no session${refusal ? `: ${JSON.stringify(refusal)}` : ""}`);
  fs.rmSync(o.out, { force: true });
  fs.writeFileSync(o.out, s.token + "\n", { mode: 0o600, flag: "wx" });
  console.log(`fixture-idp: a session for ${s.principal ?? "the person"} in ${o.out}`);
}

const [cmd, ...rest] = process.argv.slice(2);
const o = opts(rest);
if (cmd === "serve") await serve(o);
else if (cmd === "session") await session(o);
else { process.stderr.write("fixture-idp: serve | session (see the header)\n"); process.exit(1); }
