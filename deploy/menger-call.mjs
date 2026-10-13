#!/usr/bin/env node
// DEPLOY-14: one menger method over the WebSocket, from a CI runner that has
// Node and nothing else — no synapse, no npm install. deploy/record --send
// --url ws(s)://… runs it; so may a script. Node ≥ 18, its standard library
// only (a WebSocket client of ~100 lines below: the handshake, masked text
// frames, ping/pong, close), so it carries an `Authorization` header — the
// key proof — which the WHATWG WebSocket a browser or Node offers cannot.
//
//   node deploy/menger-call.mjs --url wss://menger.example.com deployments.record < params.ndjson
//   node deploy/menger-call.mjs --url ws://127.0.0.1:4455 deployments.environments --params '{"project":"PANTRY"}'
//   node deploy/menger-call.mjs --principal          # key:<hex> of $MENGER_DEPLOY_KEY: what an admin grants
//   node deploy/menger-call.mjs --new-key            # a fresh key: its seed (the CI's secret), then its principal
//
// One call per params line on stdin (or the one --params); each call's data
// items print as JSON lines, in order. A key proof (IDENTITY-40/42) signs in
// when $MENGER_DEPLOY_KEY holds a key — the 32-byte Ed25519 seed in base64,
// PLEXUS_KEY's form, `synapse _key new` makes one — naming the URL dialed
// as its audience and --agent (default `ci`) as its agent: menger lists that
// URL as the socket's (`--auth-provider key=<url>` for a tunnel's). The key
// never rides the command line. --header 'Name: value' (repeatable, or
// $MENGER_HEADERS one a line) adds a header to the upgrade — a tunnel's
// own gate (Cloudflare Access' service token, …).
// --timeout SECS (default 60): no answer by then fails the call.
// Exit: 0 done · 1 usage · 3 refused, an in-band error, or the socket failed.
import crypto from "node:crypto";
import net from "node:net";
import tls from "node:tls";
import fs from "node:fs";
import { pathToFileURL } from "node:url";

const die = (code, msg) => { process.stderr.write(`menger-call: ${msg}\n`); process.exit(code); };

// ── the key and its proof (IDENTITY-40's wire; synapse's Identity/Proof.hs) ──
const PKCS8_ED25519 = Buffer.from("302e020100300506032b657004220420", "hex");
const b64url = (b) => Buffer.from(b).toString("base64url");
export function seedOf(text) {
  const t = String(text).trim();
  const b = Buffer.from(t.replace(/-/g, "+").replace(/_/g, "/"), "base64");
  if (b.length !== 32) throw new Error(`a key is a 32-byte Ed25519 seed in base64, not ${b.length} bytes`);
  return b;
}
const privateOf = (seed) => crypto.createPrivateKey({ key: Buffer.concat([PKCS8_ED25519, seed]), format: "der", type: "pkcs8" });
export function publicHex(seed) {
  return crypto.createPublicKey(privateOf(seed)).export({ format: "der", type: "spki" }).subarray(-32).toString("hex");
}
/** `plexus-key.<base64url JSON>`, signed over `plexus-key-login/v1\n<aud>\n<agent>\n<nonce>\n<ts>`. */
export function keyProof(seed, aud, agent, nonceRaw = crypto.randomBytes(32), ts = Math.floor(Date.now() / 1000)) {
  const nonce = b64url(nonceRaw);
  const msg = ["plexus-key-login/v1", aud, agent, nonce, String(ts)].join("\n");
  const sig = crypto.sign(null, Buffer.from(msg, "utf8"), privateOf(seed));
  const json = JSON.stringify({ v: 1, key: publicHex(seed), aud, agent, nonce, ts, sig: b64url(sig) });
  return "plexus-key." + b64url(Buffer.from(json, "utf8"));
}

// ── a WebSocket client: RFC 6455, client side, text frames ──────────────────
function connect(url, headers) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const secure = u.protocol === "wss:";
    const port = Number(u.port || (secure ? 443 : 80));
    const host = u.hostname.replace(/^\[|\]$/g, "");
    const sock = secure ? tls.connect({ host, port, servername: net.isIP(host) ? undefined : host })
                        : net.connect({ host, port });
    const key = crypto.randomBytes(16).toString("base64");
    const lines = [`GET ${u.pathname || "/"}${u.search} HTTP/1.1`, `Host: ${u.host}`, "Upgrade: websocket",
      "Connection: Upgrade", `Sec-WebSocket-Key: ${key}`, "Sec-WebSocket-Version: 13",
      ...Object.entries(headers).map(([k, v]) => `${k}: ${v}`)];
    sock.once(secure ? "secureConnect" : "connect", () => sock.write(lines.join("\r\n") + "\r\n\r\n"));
    sock.once("error", (e) => reject(new Error(`cannot reach ${url}: ${e.message}`)));
    let buf = Buffer.alloc(0);
    const onHead = (chunk) => {
      buf = Buffer.concat([buf, chunk]);
      const end = buf.indexOf("\r\n\r\n");
      if (end < 0) return;
      sock.off("data", onHead);
      const head = buf.subarray(0, end).toString("latin1").split("\r\n");
      const status = head[0];
      if (!/^HTTP\/1\.1 101/.test(status)) {
        const body = buf.subarray(end + 4).toString("utf8").trim().slice(0, 300);
        const hint = / 401 /.test(status) ? " — the key was refused (is it granted, and does menger list this URL as its socket?)" : "";
        sock.destroy();
        return reject(new Error(`${url} answered ${status.replace(/^HTTP\/1\.1 /, "")}${hint}${body ? `: ${body}` : ""}`));
      }
      const accept = crypto.createHash("sha1").update(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest("base64");
      const got = head.find((l) => /^sec-websocket-accept:/i.test(l))?.split(":")[1]?.trim();
      if (got !== accept) { sock.destroy(); return reject(new Error(`${url} is not a WebSocket (bad Sec-WebSocket-Accept)`)); }
      resolve(new Socket(sock, buf.subarray(end + 4)));
    };
    sock.on("data", onHead);
  });
}

class Socket {
  constructor(sock, rest) {
    this.sock = sock; this.buf = Buffer.alloc(0); this.parts = []; this.onmessage = () => {}; this.onclose = () => {};
    sock.on("data", (c) => this.feed(c));
    sock.on("close", () => this.onclose("the socket closed"));
    sock.on("error", (e) => this.onclose(e.message));
    if (rest.length) this.feed(rest);
  }
  frame(op, payload) {
    const len = payload.length, mask = crypto.randomBytes(4);
    const head = len < 126 ? Buffer.from([0x80 | op, 0x80 | len])
      : len < 65536 ? Buffer.from([0x80 | op, 0xfe, len >> 8, len & 255])
      : Buffer.concat([Buffer.from([0x80 | op, 0xff]), (() => { const b = Buffer.alloc(8); b.writeBigUInt64BE(BigInt(len)); return b; })()]);
    const body = Buffer.from(payload);
    for (let i = 0; i < body.length; i++) body[i] ^= mask[i & 3];
    this.sock.write(Buffer.concat([head, mask, body]));
  }
  send(text) { this.frame(0x1, Buffer.from(text, "utf8")); }
  close() { try { this.frame(0x8, Buffer.from([0x03, 0xe8])); } catch {} this.sock.end(); }
  feed(chunk) {
    this.buf = Buffer.concat([this.buf, chunk]);
    for (;;) {
      if (this.buf.length < 2) return;
      const fin = this.buf[0] & 0x80, op = this.buf[0] & 0x0f, masked = this.buf[1] & 0x80;
      let len = this.buf[1] & 0x7f, off = 2;
      if (len === 126) { if (this.buf.length < 4) return; len = this.buf.readUInt16BE(2); off = 4; }
      else if (len === 127) { if (this.buf.length < 10) return; len = Number(this.buf.readBigUInt64BE(2)); off = 10; }
      const mk = masked ? this.buf.subarray(off, off + 4) : null;
      if (masked) off += 4;
      if (this.buf.length < off + len) return;
      const data = Buffer.from(this.buf.subarray(off, off + len));
      if (mk) for (let i = 0; i < data.length; i++) data[i] ^= mk[i & 3];
      this.buf = this.buf.subarray(off + len);
      if (op === 0x9) { this.frame(0xa, data); continue; }  // ping → pong
      if (op === 0xa) continue;
      if (op === 0x8) { this.sock.end(); this.onclose(`closed by menger${data.length > 2 ? `: ${data.subarray(2)}` : ""}`); continue; }
      this.parts.push(data);
      if (fin) { const msg = Buffer.concat(this.parts).toString("utf8"); this.parts = []; this.onmessage(msg); }
    }
  }
}

// ── one menger call: a `menger.call` subscription, its items until `done` ──
function call(ws, id, method, params, onItem) {
  return new Promise((resolve, reject) => {
    let sub = null;
    ws.onclose = (why) => reject(new Error(`${method}: ${why} before the call finished`));
    ws.onmessage = (text) => {
      let msg; try { msg = JSON.parse(text); } catch { return; }
      if (msg.id === id) {
        if (msg.error) return reject(new Error(`${method}: ${msg.error.message ?? JSON.stringify(msg.error)}`));
        sub = msg.result; return;
      }
      if (msg.params?.subscription === undefined || msg.params.subscription !== sub) return;
      const item = msg.params.result;
      if (item?.type === "done") return resolve();
      onItem(item);
      // A refusal (the gate's `forbidden`, …) is the stream's last item.
      if (item?.type === "error") resolve();
    };
    ws.send(JSON.stringify({ jsonrpc: "2.0", id, method: "menger.call", params: { method, params } }));
  });
}

async function main(argv) {
  let url = null, method = null, params = null, agent = process.env.MENGER_AGENT || "ci", timeout = 60;
  const headers = {};
  const addHeader = (h) => {
    const i = h.indexOf(":");
    if (i <= 0) die(1, `--header wants 'Name: value', got '${h}'`);
    headers[h.slice(0, i).trim()] = h.slice(i + 1).trim();
  };
  for (const h of (process.env.MENGER_HEADERS || "").split("\n")) if (h.trim()) addHeader(h);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i], next = () => argv[++i] ?? die(1, `${a} needs a value`);
    if (a === "--url") url = next();
    else if (a === "--params") params = next();
    else if (a === "--agent") agent = next();
    else if (a === "--timeout") { timeout = Number(next()); if (!(timeout > 0)) die(1, "--timeout is seconds, > 0"); }
    else if (a === "--header") addHeader(next());
    else if (a === "--principal" || a === "--new-key") {
      const seed = a === "--new-key" ? crypto.randomBytes(32) : seedOf(process.env.MENGER_DEPLOY_KEY ?? die(1, "--principal reads the key from $MENGER_DEPLOY_KEY"));
      if (a === "--new-key") process.stdout.write(seed.toString("base64") + "\n");
      process.stdout.write(`key:${publicHex(seed)}\n`);
      return 0;
    }
    else if (a === "-h" || a === "--help") {
      process.stdout.write(fs.readFileSync(new URL(import.meta.url), "utf8").split("\n").slice(1).filter((l) => l.startsWith("//")).map((l) => l.replace(/^\/\/ ?/, "")).join("\n") + "\n");
      return 0;
    }
    else if (!a.startsWith("-") && !method) method = a;
    else die(1, `unknown argument '${a}' (--help)`);
  }
  if (!url || !method) die(1, "usage: menger-call.mjs --url ws(s)://HOST[:PORT] METHOD [--params JSON] (--help)");
  if (!/^wss?:\/\//.test(url)) die(1, `--url ${url}: the WebSocket is ws:// or wss:// (the REST bridge is curl's: deploy/record --url http…)`);
  const calls = (params !== null ? params : fs.readFileSync(0, "utf8")).split("\n").filter((l) => l.trim());
  if (calls.length === 0) calls.push("{}");
  const parsed = calls.map((l) => { try { return JSON.parse(l); } catch { die(1, `params are not JSON: ${l.slice(0, 120)}`); } });

  const aud = url.replace(/\/+$/, "");
  if (process.env.MENGER_DEPLOY_KEY) {
    let seed; try { seed = seedOf(process.env.MENGER_DEPLOY_KEY); } catch (e) { die(1, `MENGER_DEPLOY_KEY: ${e.message}`); }
    if (!/^[A-Za-z0-9._-]{1,64}$/.test(agent)) die(1, `--agent '${agent}': 1–64 of A-Z a-z 0-9 . _ -`);
    headers.Authorization = `Bearer ${keyProof(seed, aud, agent)}`;
  }
  // A CI step never hangs: no answer within --timeout (default 60 s) fails it.
  setTimeout(() => die(3, `${url}: no answer within ${timeout} s`), timeout * 1000).unref();
  let ws;
  try { ws = await connect(url, headers); } catch (e) { die(3, e.message); }
  let failed = false;
  try {
    for (let i = 0; i < parsed.length; i++) {
      await call(ws, i + 1, method, parsed[i], (item) => {
        if (item?.type === "data") {
          process.stdout.write(JSON.stringify(item.content) + "\n");
          if (item.content?.type === "error") failed = true;
        } else if (item?.type === "error") {
          const { metadata, ...refusal } = item;
          process.stdout.write(JSON.stringify(refusal) + "\n");
          failed = true;
        }
      });
    }
  } catch (e) { ws.close(); die(3, e.message); }
  ws.close();
  return failed ? 3 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(fs.realpathSync(process.argv[1])).href) {
  main(process.argv.slice(2)).then((code) => process.exit(code), (e) => die(3, e.message));
}
