#!/usr/bin/env node
// ACCESS-29: a flood of upgrades and logins leaves menger's memory
// flat.
//
// (The calls one connection may hold, --max-calls-per-connection, are a
// signed-in caller's: an anonymous socket's calls are the gate's refusals.
// tests/wire_limits.rs holds them.)
//
// Starts a menger (a throwaway database, login on through a fixture OIDC
// issuer this script serves and nostr, behind a "proxy" — --trusted-proxy
// 127.0.0.1 — so each request can name its own client in X-Forwarded-For),
// then floods it in rounds:
//
//   upgrades  anonymous WebSockets opened at once, far past AUTH-12's
//             quotas (128 anonymous, 32 per IP): the rest refused 429;
//   logins    GET /login/start (a login transaction each, capped 10,000),
//             POST /login (a password relayed to the issuer, reserved per
//             account first) and POST /login/nostr (refused events), each
//             from its own address — every throttle table filling to its
//             cap (100,000) and evicting.
//
// After each round it reads the server's resident memory (VmRSS). The
// caps fill in the first rounds; past them, every round must leave memory
// where the one before did: the last round's RSS within --tolerance (10%)
// of the round at --settle (4). Exit 0 when flat, 1 when it grew.
//
//   node flood.mjs --bin ../target/release/menger          # 6 rounds
//   node flood.mjs --bin $CARGO_TARGET_DIR/debug/menger --rounds 4 --logins 20000
//
// Options: --bin PATH (default ../target/release/menger) · --port N (the
// WebSocket; the shell is N+1, the issuer N+3; default 4630) · --rounds N ·
// --upgrades N (per round, default 2000) ·
// --logins N (per kind per round, default 30000) · --settle N ·
// --tolerance X · --keep-db.

import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => {
    if (a.startsWith("--")) acc.push([a.slice(2), all[i + 1] && !all[i + 1].startsWith("--") ? all[i + 1] : true]);
    return acc;
  }, []),
);
const HERE = path.dirname(new URL(import.meta.url).pathname);
const BIN = path.resolve(args.bin ?? path.join(HERE, "..", "target", "release", "menger"));
const WS = Number(args.port ?? 4630);
const SHELL = WS + 1;
const ISSUER = WS + 3;
const ROUNDS = Number(args.rounds ?? 6);
const UPGRADES = Number(args.upgrades ?? 2000);
const LOGINS = Number(args.logins ?? 30000);
const SETTLE = Number(args.settle ?? 4);
const TOLERANCE = Number(args.tolerance ?? 0.1);

// ── the fixture issuer: discovery, an empty JWKS, a token endpoint that
// refuses every password ────────────────────────────────────────────────
const base = `http://127.0.0.1:${ISSUER}`;
const issuer = http.createServer((req, res) => {
  const json = (v, status = 200) => {
    res.writeHead(status, { "content-type": "application/json" });
    res.end(JSON.stringify(v));
  };
  if (req.url.startsWith("/.well-known/openid-configuration"))
    return json({ issuer: base, jwks_uri: `${base}/jwks.json`, authorization_endpoint: `${base}/authorize`, token_endpoint: `${base}/token` });
  if (req.url.startsWith("/jwks.json")) return json({ keys: [] });
  if (req.url.startsWith("/token")) {
    req.resume();
    return req.on("end", () => json({ error: "invalid_grant" }, 400));
  }
  json({ error: "not_found" }, 404);
});
await new Promise((ok) => issuer.listen(ISSUER, "127.0.0.1", ok));

// ── menger ─────────────────────────────────────────────────────────────
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "menger-flood-"));
const server = spawn(
  BIN,
  [
    "--port", String(WS), "--shell-port", String(SHELL), "--no-rest", "--db-path", path.join(dir, "t.db"),
    "--auth-provider", `idp=${base},flood:menger`,
    "--auth-provider", `auth0=${base},https://menger.flood,client=flood`,
    "--auth-provider", `nostr=ws://127.0.0.1:${WS}/`,
    "--login-button", "Google=auth0:google-oauth2",
    "--trusted-proxy", "127.0.0.1",
  ],
  { env: { ...process.env, PLEXUS_NO_REGISTRY: "1", RUST_LOG: "warn" }, stdio: ["ignore", "ignore", "pipe"] },
);
let stderr = "";
server.stderr.on("data", (d) => (stderr = (stderr + d).slice(-4000)));
const stop = () => {
  server.kill("SIGTERM");
  issuer.close();
  if (!args["keep-db"]) fs.rmSync(dir, { recursive: true, force: true });
};
process.on("exit", stop);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; ; i++) {
  try {
    await fetch(`http://127.0.0.1:${SHELL}/login`);
    break;
  } catch {
    if (i > 200 || server.exitCode !== null) {
      console.error(`menger did not come up:\n${stderr}`);
      process.exit(2);
    }
    await sleep(100);
  }
}

const rss = () => {
  const s = fs.readFileSync(`/proc/${server.pid}/status`, "utf8");
  return Number(/VmRSS:\s+(\d+)/.exec(s)[1]) / 1024;
};

/** A client address: request i's own /16 (one of 223 × 256), round r's
 *  /24 inside it — every request its own network at every level, so each
 *  throttle table fills (the worst case for memory), and no neighbourhood
 *  or provider allowance refuses the flood for it. */
const ip = (r, i) => `${1 + (i % 223)}.${Math.floor(i / 223) % 256}.${r % 256}.1`;

/** Runs `n` jobs, `width` at a time. */
async function pool(n, width, job) {
  let next = 0;
  const counts = {};
  await Promise.all(
    Array.from({ length: width }, async () => {
      while (next < n) {
        const k = await job(next++).catch((e) => `error ${e.cause?.code ?? e.message}`);
        counts[k] = (counts[k] ?? 0) + 1;
      }
    }),
  );
  return counts;
}

async function upgrades(n) {
  const opened = [];
  const counts = { open: 0, refused: 0 };
  await Promise.all(
    Array.from({ length: n }, () =>
      new Promise((ok) => {
        const ws = new WebSocket(`ws://127.0.0.1:${WS}`);
        ws.onopen = () => (counts.open++, opened.push(ws), ok());
        ws.onerror = () => (counts.refused++, ok());
      }),
    ),
  );
  for (const ws of opened) ws.close();
  await sleep(300);
  return counts;
}

async function logins(r, n) {
  const start = await pool(n, 64, async (i) => {
    const res = await fetch(`http://127.0.0.1:${SHELL}/login/start?button=Google&next=/`, { redirect: "manual", headers: { "x-forwarded-for": ip(r, i) } });
    await res.arrayBuffer();
    return `start ${res.status}`;
  });
  const password = await pool(n, 64, async (i) => {
    const res = await fetch(`http://127.0.0.1:${SHELL}/login`, {
      method: "POST",
      redirect: "manual",
      headers: { "x-forwarded-for": ip(r, i), "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ username: `user-${r}-${i}`, password: "guess", next: "/" }),
    });
    await res.arrayBuffer();
    return `password ${res.status}`;
  });
  const nostr = await pool(n, 64, async (i) => {
    const res = await fetch(`http://127.0.0.1:${SHELL}/login/nostr`, {
      method: "POST",
      headers: { "x-forwarded-for": ip(r, i), authorization: "Nostr e30=", accept: "application/json" },
    });
    await res.arrayBuffer();
    return `nostr ${res.status}`;
  });
  // One client, over and over: refused once its flows are spent.
  const one = await pool(200, 8, async () => {
    const res = await fetch(`http://127.0.0.1:${SHELL}/login/start?button=Google&next=/`, { redirect: "manual", headers: { "x-forwarded-for": "192.0.2.7" } });
    await res.arrayBuffer();
    return `one client ${res.status}`;
  });
  return { ...start, ...password, ...nostr, ...one };
}

const rows = [{ round: 0, rss_mb: rss().toFixed(1), what: "started" }];
console.log(`menger ${BIN} (pid ${server.pid}) on ws ${WS}, shell ${SHELL}; ${ROUNDS} rounds`);
console.log(`round 0: ${rows[0].rss_mb} MB`);
for (let r = 1; r <= ROUNDS; r++) {
  const u = await upgrades(UPGRADES);
  const l = await logins(r, LOGINS);
  await sleep(500);
  const row = { round: r, rss_mb: rss().toFixed(1), upgrades: u, logins: l };
  rows.push(row);
  console.log(`round ${r}: ${row.rss_mb} MB — upgrades ${JSON.stringify(u)} · logins ${JSON.stringify(l)}`);
}

const settled = Number(rows[Math.min(SETTLE, ROUNDS)].rss_mb);
const last = Number(rows[ROUNDS].rss_mb);
const growth = (last - settled) / settled;
const flat = growth <= TOLERANCE;
console.log(
  `${flat ? "flat" : "GREW"}: ${settled} MB after round ${Math.min(SETTLE, ROUNDS)}, ${last} MB after round ${ROUNDS} ` +
    `(${(growth * 100).toFixed(1)}%, tolerance ${(TOLERANCE * 100).toFixed(0)}%)`,
);
fs.writeFileSync(path.join(dir, "..", `menger-flood-${process.pid}.json`), JSON.stringify(rows, null, 2));
process.exit(flat ? 0 : 1);
